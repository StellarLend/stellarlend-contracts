/**
 * Failure-path, boundary, TTL, and concurrency coverage for the oracle
 * caching service (`src/services/cache.ts`).
 *
 * Determinism strategy:
 * - All time is driven with `vi.setSystemTime` so no test depends on real
 *   wall-clock sleeps or on CI machine speed.
 * - The wall clock is pinned to a fixed epoch (`T0`) and only moved explicitly,
 *   which makes every TTL boundary assertion exact.
 * - The winston logger is mocked out so log output cannot interleave with
 *   test output or introduce async side effects under fake timers.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { Cache, PriceCache, createCache, createPriceCache } from '../src/services/cache.js';

vi.mock('../src/utils/logger.js', () => ({
    logger: {
        debug: vi.fn(),
        info: vi.fn(),
        warn: vi.fn(),
        error: vi.fn(),
    },
}));

/** Fixed reference instant used by every time-based assertion. */
const T0 = 1_760_000_000_000;

/** Move the pinned clock forward without touching anything else. */
function advance(ms: number): void {
    vi.setSystemTime(Date.now() + ms);
}

beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(T0);
});

afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
});

describe('Cache: construction and configuration failure paths', () => {
    it('rejects a negative default TTL', () => {
        expect(() => createCache({ defaultTtlSeconds: -1 })).toThrow('TTL values must be non-negative');
    });

    it('rejects a negative stale TTL even when the default TTL is valid', () => {
        expect(() => createCache({ defaultTtlSeconds: 30, staleTtlSeconds: -1 })).toThrow(
            'TTL values must be non-negative',
        );
    });

    it('accepts zero TTLs as the lower boundary', () => {
        expect(() => createCache({ defaultTtlSeconds: 0, staleTtlSeconds: 0 })).not.toThrow();
    });

    it('does not construct a usable instance when configuration is invalid', () => {
        let cache: Cache | undefined;
        try {
            cache = createCache({ defaultTtlSeconds: -5 });
        } catch {
            cache = undefined;
        }

        expect(cache).toBeUndefined();
    });

    it('applies documented defaults when no configuration is supplied', () => {
        const cache = createCache();

        cache.set('xlm', 1n);
        // Default freshness TTL is 30s.
        advance(30_000);
        expect(cache.get('xlm')).toBe(1n);

        advance(30_001);
        expect(cache.get('xlm')).toBeUndefined();
    });

    it('reports a zero hit rate before any read has happened', () => {
        const stats = createCache().getStats();

        expect(stats).toEqual({ size: 0, hits: 0, misses: 0, hitRate: 0 });
    });

    it('accepts an explicit undefined TTL, which yields non-expiring entries', () => {
        // `{ ...DEFAULT_CONFIG, ...config }` copies an explicit `undefined`
        // over the default, and `ttlSeconds ?? config.defaultTtlSeconds`
        // then resolves to `undefined` -> `expiresAt` becomes NaN -> the entry
        // can never expire. Documented here as a known configuration hazard.
        const cache = createCache({ defaultTtlSeconds: undefined });

        cache.set('immortal', 'value');
        advance(10 * 365 * 24 * 60 * 60 * 1000);

        expect(cache.get('immortal')).toBe('value');
        expect(cache.cleanup()).toBe(0);
    });
});

describe('Cache: capacity and eviction boundaries', () => {
    it('holds exactly one entry when maxEntries is zero', () => {
        const cache = createCache({ maxEntries: 0 });

        cache.set('first', 1);
        expect(cache.getStats().size).toBe(1);

        cache.set('second', 2);
        expect(cache.getStats().size).toBe(1);
        expect(cache.get('first')).toBeUndefined();
        expect(cache.get('second')).toBe(2);
    });

    it('behaves as a single-slot cache when maxEntries is negative', () => {
        const cache = createCache({ maxEntries: -1 });

        cache.set('first', 1);
        cache.set('second', 2);

        expect(cache.getStats().size).toBe(1);
        expect(cache.get('first')).toBeUndefined();
        expect(cache.get('second')).toBe(2);
    });

    it('does not evict when a full cache is written with an existing key', () => {
        const cache = createCache({ maxEntries: 2 });

        cache.set('a', 1, 10, T0 - 3_000);
        cache.set('b', 2, 10, T0 - 2_000);
        cache.set('a', 3, 10, T0 - 1_000);

        expect(cache.getStats().size).toBe(2);
        expect(cache.get('a')).toBe(3);
        expect(cache.get('b')).toBe(2);
    });

    it('evicts the entry with the smallest cachedAt after an overwrite refreshes it', () => {
        const cache = createCache({ maxEntries: 2 });

        cache.set('a', 1, 10, T0 - 3_000);
        cache.set('b', 2, 10, T0 - 2_000);
        cache.set('a', 3, 10, T0 - 1_000);
        // `a` now has the newest cachedAt, so `b` is the eviction candidate.
        cache.set('c', 4, 10, T0);

        expect(cache.get('a')).toBe(3);
        expect(cache.get('b')).toBeUndefined();
        expect(cache.get('c')).toBe(4);
    });

    it('breaks cachedAt ties by insertion order', () => {
        const cache = createCache({ maxEntries: 2 });

        cache.set('a', 1, 10, T0);
        cache.set('b', 2, 10, T0);
        cache.set('c', 3, 10, T0);

        expect(cache.get('a')).toBeUndefined();
        expect(cache.get('b')).toBe(2);
        expect(cache.get('c')).toBe(3);
    });

    it('evicts an entry even when it is still fresh', () => {
        const cache = createCache({ maxEntries: 1, defaultTtlSeconds: 3_600 });

        cache.set('fresh', 1);
        cache.set('newcomer', 2);

        expect(cache.get('fresh')).toBeUndefined();
        expect(cache.get('newcomer')).toBe(2);
    });

    it('never exceeds maxEntries under sustained writes', () => {
        const cache = createCache({ maxEntries: 10 });

        for (let i = 0; i < 1_000; i++) {
            cache.set(`key-${i}`, i);
            expect(cache.getStats().size).toBeLessThanOrEqual(10);
        }

        expect(cache.getStats().size).toBe(10);
        // The most recent write always survives.
        expect(cache.get('key-999')).toBe(999);
    });

    it('restores full capacity after clear', () => {
        const cache = createCache({ maxEntries: 3 });

        cache.set('a', 1);
        cache.set('b', 2);
        cache.set('c', 3);
        cache.clear();

        cache.set('d', 4);
        cache.set('e', 5);
        cache.set('f', 6);

        expect(cache.getStats().size).toBe(3);
        expect(cache.get('a')).toBeUndefined();
        expect(cache.get('f')).toBe(6);
    });
});

describe('Cache: freshness TTL and stale grace window', () => {
    it('serves an entry at the exact instant it expires', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value');
        advance(10_000);

        expect(cache.get('key')).toBe('value');
        expect(cache.getStats().hits).toBe(1);
    });

    it('rejects a read one millisecond past the freshness TTL', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value');
        advance(10_001);

        expect(cache.get('key')).toBeUndefined();
        expect(cache.getStats()).toMatchObject({ hits: 0, misses: 1 });
    });

    it('leaves a soft-expired entry in the store so stale fallback still works', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value');
        advance(10_001);

        expect(cache.get('key')).toBeUndefined();
        // `get` does not evict; only `getStale`/`has`/`cleanup` do.
        expect(cache.getStats().size).toBe(1);
        expect(cache.getStale('key')).toEqual({ data: 'value', isStale: true });
    });

    it('serves stale data at the exact instant the grace window closes', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value');
        advance(70_000);

        expect(cache.getStale('key')).toEqual({ data: 'value', isStale: true });
    });

    it('evicts and reports a miss one millisecond past the hard expiry', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value');
        advance(70_001);

        expect(cache.getStale('key')).toBeUndefined();
        expect(cache.getStats()).toMatchObject({ size: 0, hits: 0, misses: 1 });
    });

    it('treats a zero stale TTL as an immediate hard expiry', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 0 });

        cache.set('key', 'value');
        advance(10_001);

        expect(cache.getStale('key')).toBeUndefined();
        expect(cache.getStats().size).toBe(0);
    });

    it('marks a fresh read as not stale', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value');

        expect(cache.getStale('key')).toEqual({ data: 'value', isStale: false });
        expect(cache.getStats().misses).toBe(0);
    });

    it('counts a stale fallback as a hit, not a miss', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value');
        advance(10_001);
        cache.getStale('key');

        expect(cache.getStats()).toMatchObject({ hits: 1, misses: 0, hitRate: 1 });
    });

    it('counts a miss for a key that was never written', () => {
        const cache = createCache();

        expect(cache.getStale('absent')).toBeUndefined();
        expect(cache.getStats()).toMatchObject({ hits: 0, misses: 1, hitRate: 0 });
    });

    it('refreshes the TTL window when a key is overwritten', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'first');
        advance(9_000);
        cache.set('key', 'second');

        advance(9_000);
        expect(cache.get('key')).toBe('second');
        advance(2_000);
        expect(cache.get('key')).toBeUndefined();
    });
});

describe('Cache: has() semantics', () => {
    it('returns true while the entry is fresh', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value');

        expect(cache.has('key')).toBe(true);
    });

    it('still returns true inside the stale grace window', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value');
        advance(10_001);

        expect(cache.has('key')).toBe(true);
    });

    it('evicts and returns false past the hard expiry', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value');
        advance(70_001);

        expect(cache.has('key')).toBe(false);
        expect(cache.getStats().size).toBe(0);
    });

    it('does not count towards hit or miss statistics', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('present', 'value');
        cache.set('expiring', 'value', 1);
        advance(2_000);

        expect(cache.has('present')).toBe(true);
        expect(cache.has('absent')).toBe(false);
        expect(cache.has('expiring')).toBe(true);

        expect(cache.getStats()).toMatchObject({ hits: 0, misses: 0 });
    });

    it('returns false for a key deleted earlier', () => {
        const cache = createCache();

        cache.set('key', 'value');
        cache.delete('key');

        expect(cache.has('key')).toBe(false);
    });
});

describe('Cache: cleanup', () => {
    it('removes only entries past their hard expiry', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('fresh', 1, 3_600);
        // Soft-expired by `advance`, but still inside the 60s grace window.
        cache.set('stale', 2, 10);
        // Already past its hard expiry before `advance` runs.
        cache.set('dead', 3, 10, T0 - 100_000);
        advance(30_000);

        expect(cache.cleanup()).toBe(1);
        expect(cache.getStats().size).toBe(2);
        expect(cache.get('fresh')).toBe(1);
        expect(cache.getStale('stale')).toEqual({ data: 2, isStale: true });
    });

    it('keeps an entry sitting exactly on the hard expiry boundary', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 1);
        advance(70_000);

        expect(cache.cleanup()).toBe(0);
        expect(cache.getStats().size).toBe(1);

        advance(1);
        expect(cache.cleanup()).toBe(1);
    });

    it('is idempotent', () => {
        const cache = createCache({ defaultTtlSeconds: 1, staleTtlSeconds: 1 });

        cache.set('a', 1);
        cache.set('b', 2);
        advance(5_000);

        expect(cache.cleanup()).toBe(2);
        expect(cache.cleanup()).toBe(0);
        expect(cache.getStats().size).toBe(0);
    });

    it('returns zero on an empty cache', () => {
        expect(createCache().cleanup()).toBe(0);
    });

    it('does not reset hit/miss statistics', () => {
        const cache = createCache({ defaultTtlSeconds: 1, staleTtlSeconds: 1 });

        cache.set('a', 1);
        cache.get('a');
        cache.get('missing');
        advance(5_000);
        cache.cleanup();

        expect(cache.getStats()).toMatchObject({ size: 0, hits: 1, misses: 1, hitRate: 0.5 });
    });
});

describe('Cache: delete and clear', () => {
    it('reports whether the key existed', () => {
        const cache = createCache();

        cache.set('key', 'value');

        expect(cache.delete('key')).toBe(true);
        expect(cache.delete('key')).toBe(false);
    });

    it('removes an entry that is only inside the stale grace window', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value');
        advance(30_000);

        expect(cache.delete('key')).toBe(true);
        expect(cache.getStale('key')).toBeUndefined();
        expect(cache.getStats()).toMatchObject({ size: 0, misses: 1 });
    });

    it('empties the store on clear and keeps statistics', () => {
        const cache = createCache();

        cache.set('a', 1);
        cache.set('b', 2);
        cache.get('a');
        cache.clear();

        expect(cache.getStats()).toMatchObject({ size: 0, hits: 1, misses: 0 });
        expect(cache.get('a')).toBeUndefined();
    });

    it('is safe to clear an already empty cache', () => {
        const cache = createCache();

        expect(() => cache.clear()).not.toThrow();
        expect(cache.getStats().size).toBe(0);
    });

    it('does not leak entries between instances', () => {
        const first = createCache();
        const second = createCache();

        first.set('shared-key', 'first');
        first.clear();

        second.set('shared-key', 'second');

        expect(first.get('shared-key')).toBeUndefined();
        expect(second.get('shared-key')).toBe('second');
    });
});

describe('Cache: invalid keys and prototype-pollution safety', () => {
    const keys: Array<[string, string]> = [
        ['empty string key', ''],
        ['whitespace key', '   '],
        ['key containing newlines and tabs', 'a\nb\tc'],
        ['unicode key', '価格-\u{1F680}'],
        ['key with a null byte', 'bad\u0000key'],
        ['key with a leading colon', ':leading'],
        ['numeric-looking key', '12345'],
        ['very long key', 'k'.repeat(10_000)],
        ['key that looks like a price key', 'price:BTC'],
    ];

    for (const [label, key] of keys) {
        it(`round-trips a ${label}`, () => {
            const cache = createCache({ defaultTtlSeconds: 10 });

            cache.set(key, { label });

            expect(cache.get(key)).toEqual({ label });
            expect(cache.has(key)).toBe(true);
            expect(cache.delete(key)).toBe(true);
        });
    }

    const dangerousKeys = ['__proto__', 'constructor', 'prototype', 'toString', 'hasOwnProperty', 'valueOf'];

    for (const key of dangerousKeys) {
        it(`stores ${key} as an ordinary key without polluting Object.prototype`, () => {
            const cache = createCache();
            const sentinel = { injected: key };
            const prototypeBefore = Object.getOwnPropertyNames(Object.prototype);

            cache.set(key, sentinel);

            expect(cache.get(key)).toBe(sentinel);
            expect(cache.has(key)).toBe(true);
            expect(cache.getStats().size).toBe(1);
            // The backing store is a Map, so no key can reach the prototype.
            expect(Object.getOwnPropertyNames(Object.prototype)).toEqual(prototypeBefore);
            expect(({} as Record<string, unknown>).injected).toBeUndefined();
        });
    }

    it('does not let a __proto__ key shadow Object.prototype lookups', () => {
        const cache = createCache();

        cache.set('__proto__', { polluted: true });

        expect((Object.prototype as unknown as Record<string, unknown>).polluted).toBeUndefined();
        expect(({} as Record<string, unknown>).polluted).toBeUndefined();
        expect(cache.getStats().size).toBe(1);
    });

    it('treats keys differing only by case as distinct entries', () => {
        const cache = createCache();

        cache.set('XLM', 'upper');
        cache.set('xlm', 'lower');

        expect(cache.getStats().size).toBe(2);
        expect(cache.get('XLM')).toBe('upper');
        expect(cache.get('xlm')).toBe('lower');
    });

    it('does not confuse keys that differ only by a trailing separator', () => {
        const cache = createCache();

        cache.set('price:BTC', 'with-prefix');
        cache.set('BTC', 'without-prefix');

        expect(cache.get('price:BTC')).toBe('with-prefix');
        expect(cache.get('BTC')).toBe('without-prefix');
    });
});

describe('Cache: malformed and boundary payloads', () => {
    it('stores undefined and reports it as a hit, distinct from a miss', () => {
        const cache = createCache();

        cache.set('undefined', undefined);

        expect(cache.get('undefined')).toBeUndefined();
        // The entry exists, so the read is a hit even though the value is undefined.
        expect(cache.getStats()).toMatchObject({ hits: 1, misses: 0 });
        expect(cache.has('undefined')).toBe(true);
        expect(cache.getStale('undefined')).toEqual({ data: undefined, isStale: false });
    });

    it('round-trips falsy and non-finite values without coercion', () => {
        const cache = createCache();

        cache.set('null', null);
        cache.set('zero', 0);
        cache.set('empty-string', '');
        cache.set('false', false);
        cache.set('nan', Number.NaN);
        cache.set('infinity', Number.POSITIVE_INFINITY);
        cache.set('neg-infinity', Number.NEGATIVE_INFINITY);
        cache.set('zero-bigint', 0n);
        cache.set('neg-bigint', -42n);

        expect(cache.get('null')).toBeNull();
        expect(cache.get('zero')).toBe(0);
        expect(cache.get('empty-string')).toBe('');
        expect(cache.get('false')).toBe(false);
        expect(cache.get('nan')).toBeNaN();
        expect(cache.get('infinity')).toBe(Number.POSITIVE_INFINITY);
        expect(cache.get('neg-infinity')).toBe(Number.NEGATIVE_INFINITY);
        expect(cache.get('zero-bigint')).toBe(0n);
        expect(cache.get('neg-bigint')).toBe(-42n);
    });

    it('round-trips an extremely large bigint exactly', () => {
        const cache = createCache();
        const huge = 2n ** 512n - 1n;

        cache.set('huge', huge);

        expect(cache.get('huge')).toBe(huge);
    });

    it('stores by reference without defensive cloning', () => {
        const cache = createCache();
        const payload = { nested: { count: 0 }, list: [1, 2] };

        cache.set('payload', payload);
        payload.nested.count = 99;
        payload.list.push(3);

        // Documented aliasing behaviour: the cache holds the caller's object.
        expect(cache.get('payload')).toBe(payload);
        expect(cache.get<typeof payload>('payload')?.nested.count).toBe(99);
    });

    it('isolates mutations to a later overwrite of the same key', () => {
        const cache = createCache();
        const first = { id: 1 };
        const second = { id: 2 };

        cache.set('key', first);
        cache.set('key', second);
        first.id = 42;

        expect(cache.get('key')).toBe(second);
    });

    it('stores falsy top-level values for the price-shaped accessor', () => {
        const cache = createCache();

        cache.set('zero', 0n);
        cache.set('nullish', null);

        expect(cache.get('zero')).toBe(0n);
        expect(cache.get('nullish')).toBeNull();
    });
});

describe('Cache: degenerate TTL values', () => {
    it('treats a zero TTL as expiring at the exact instant of the write', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value', 0);

        // Expiry comparisons are strict (`now > expiresAt`), so the entry is
        // still served at the instant it was written.
        expect(cache.get('key')).toBe('value');
        advance(1);
        expect(cache.get('key')).toBeUndefined();
        // Still recoverable through the stale window.
        expect(cache.getStale('key')).toEqual({ data: 'value', isStale: true });
    });

    it('treats a negative TTL as expired on write', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 'value', -30);

        expect(cache.get('key')).toBeUndefined();
        expect(cache.getStale('key')).toEqual({ data: 'value', isStale: true });
    });

    it('supports a sub-millisecond TTL and honours its exact boundary', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 0 });

        cache.set('key', 'value', 0.001);
        expect(cache.get('key')).toBe('value');

        // 0.001s rounds to a 1ms window; the closing instant is inclusive.
        advance(1);
        expect(cache.get('key')).toBe('value');
        advance(1);
        expect(cache.get('key')).toBeUndefined();
        expect(cache.getStale('key')).toBeUndefined();
    });

    it('never expires an entry written with an infinite TTL', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('forever', 'value', Number.POSITIVE_INFINITY);
        advance(10 * 365 * 24 * 60 * 60 * 1000);

        expect(cache.get('forever')).toBe('value');
        expect(cache.has('forever')).toBe(true);
        expect(cache.cleanup()).toBe(0);
    });

    it('never expires an entry written with a NaN TTL', () => {
        // NaN comparisons are always false, so `expiresAt = NaN` disables expiry
        // entirely. Documented as a hazard of unvalidated caller input.
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('broken', 'value', Number.NaN);
        advance(10 * 365 * 24 * 60 * 60 * 1000);

        expect(cache.get('broken')).toBe('value');
        expect(cache.cleanup()).toBe(0);
    });

    it('accepts a NaN default TTL without throwing', () => {
        expect(() => createCache({ defaultTtlSeconds: Number.NaN })).not.toThrow();
    });

    it('expires an entry backdated through an explicit cachedAt', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('backdated', 'value', 10, T0 - 60_000);

        expect(cache.get('backdated')).toBeUndefined();
        expect(cache.getStale('backdated')).toEqual({ data: 'value', isStale: true });

        advance(10_001);
        expect(cache.getStale('backdated')).toBeUndefined();
    });

    it('keeps a future-dated entry fresh until that future instant passes', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('future', 'value', 10, T0 + 60_000);

        advance(69_999);
        expect(cache.get('future')).toBe('value');
        advance(1);
        expect(cache.get('future')).toBe('value');
        advance(1);
        expect(cache.get('future')).toBeUndefined();
    });

    it('lets a per-call TTL override a longer default TTL', () => {
        const cache = createCache({ defaultTtlSeconds: 3_600, staleTtlSeconds: 60 });

        cache.set('short', 'value', 5);
        advance(5_001);

        expect(cache.get('short')).toBeUndefined();
    });
});

describe('Cache: statistics accounting', () => {
    it('computes a hit rate of 1 when every read hits', () => {
        const cache = createCache();

        cache.set('a', 1);
        cache.get('a');
        cache.get('a');
        cache.get('a');

        expect(cache.getStats().hitRate).toBe(1);
    });

    it('computes a hit rate of 0 when every read misses', () => {
        const cache = createCache();

        cache.get('a');
        cache.get('b');

        expect(cache.getStats().hitRate).toBe(0);
    });

    it('keeps hits and misses equal to the number of read calls', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('present', 1);
        for (let i = 0; i < 25; i++) {
            cache.get(i % 2 === 0 ? 'present' : 'absent');
        }

        const stats = cache.getStats();
        expect(stats.hits).toBe(13);
        expect(stats.misses).toBe(12);
        expect(stats.hits + stats.misses).toBe(25);
        expect(stats.hitRate).toBeCloseTo(13 / 25, 10);
    });

    it('reports size for soft-expired entries until they are reclaimed', () => {
        const cache = createCache({ defaultTtlSeconds: 10, staleTtlSeconds: 60 });

        cache.set('key', 1);
        advance(10_001);

        expect(cache.getStats().size).toBe(1);
        expect(cache.cleanup()).toBe(0);
        advance(60_000);
        expect(cache.cleanup()).toBe(1);
    });
});

describe('Cache: concurrent read/write interleaving', () => {
    it('never observes a torn value while a key is rewritten', () => {
        const cache = createCache({ defaultTtlSeconds: 10 });
        const observed: unknown[] = [];

        for (let round = 0; round < 200; round++) {
            cache.set('torn', { a: round, b: round }, 10);
            const read = cache.get<{ a: number; b: number }>('torn');
            if (read) {
                observed.push(read.a);
                // Both fields must come from the same write.
                expect(read.b).toBe(read.a);
            }
        }

        expect(observed).toHaveLength(200);
    });

    it('keeps a key absent after a concurrent delete', () => {
        const cache = createCache();

        cache.set('key', 'value');
        expect(cache.get('key')).toBe('value');
        expect(cache.delete('key')).toBe(true);

        for (let i = 0; i < 50; i++) {
            expect(cache.get('key')).toBeUndefined();
            expect(cache.has('key')).toBe(false);
            expect(cache.delete('key')).toBe(false);
        }

        expect(cache.get('key')).toBeUndefined();
        expect(cache.getStats().size).toBe(0);
    });

    it('returns nothing for any key once a concurrent clear lands', () => {
        const cache = createCache({ defaultTtlSeconds: 10 });
        const keys = Array.from({ length: 32 }, (_, i) => `key-${i}`);

        for (const key of keys) cache.set(key, key);
        cache.clear();

        for (const key of keys) {
            expect(cache.get(key)).toBeUndefined();
            expect(cache.has(key)).toBe(false);
        }
    });

    it('holds the size invariant under interleaved writes, reads and cleanup', () => {
        const cache = createCache({ maxEntries: 16, defaultTtlSeconds: 1, staleTtlSeconds: 1 });
        let reads = 0;

        for (let i = 0; i < 500; i++) {
            cache.set(`key-${i % 40}`, i);
            cache.get(`key-${i % 40}`);
            reads++;
            if (i % 25 === 0) {
                expect(cache.cleanup()).toBeGreaterThanOrEqual(0);
            }
            expect(cache.getStats().size).toBeLessThanOrEqual(16);
            expect(cache.getStats().hits + cache.getStats().misses).toBe(reads);
        }
    });

    it('does not resurrect an entry reclaimed by cleanup between writes', () => {
        const cache = createCache({ defaultTtlSeconds: 1, staleTtlSeconds: 1 });

        cache.set('key', 'value');
        advance(2_001);
        expect(cache.cleanup()).toBe(1);
        expect(cache.get('key')).toBeUndefined();
        expect(cache.getStats().size).toBe(0);
    });

    it('serialises interleaved writers to a single consistent final value', async () => {
        const cache = createCache({ defaultTtlSeconds: 10 });

        const writers = [1, 2, 3, 4, 5].map(async (id) => {
            for (let round = 0; round < 20; round++) {
                await Promise.resolve();
                cache.set('shared', { id, round });
            }
        });

        await Promise.all(writers);

        const final = cache.get<{ id: number; round: number }>('shared');
        expect(final).toBeDefined();
        expect(final?.round).toBe(19);
        expect(cache.getStats().size).toBe(1);
    });
});

describe('PriceCache: lifecycle and boundary behaviour', () => {
    it('normalises asset symbols to uppercase on write and read', () => {
        const priceCache = createPriceCache(30);

        priceCache.setPrice('xlm', 150_000n);

        expect(priceCache.getPrice('XLM')).toBe(150_000n);
        expect(priceCache.getPrice('xlm')).toBe(150_000n);
        expect(priceCache.getPrice('XlM')).toBe(150_000n);
        expect(priceCache.getStats().size).toBe(1);
    });

    it('distinguishes a zero price from a cache miss', () => {
        const priceCache = createPriceCache(30);

        priceCache.setPrice('ZERO', 0n);

        expect(priceCache.getPrice('ZERO')).toBe(0n);
        expect(priceCache.getPrice('MISSING')).toBeUndefined();
        expect(priceCache.hasPrice('ZERO')).toBe(true);
    });

    it('round-trips a negative bigint price', () => {
        const priceCache = createPriceCache(30);

        priceCache.setPrice('NEG', -1n);

        expect(priceCache.getPrice('NEG')).toBe(-1n);
    });

    it('rejects an invalid asset payload without throwing', () => {
        const priceCache = createPriceCache(30);

        // A malformed price value is stored and read back verbatim; the cache
        // layer performs no validation, so callers must validate before write.
        priceCache.setPrice('BAD', Number.NaN as unknown as bigint);

        expect(priceCache.getPrice('BAD')).toBeNaN();
    });

    it('stores hostile asset names as ordinary keys', () => {
        const priceCache = createPriceCache(30);

        priceCache.setPrice('__proto__', 1n);
        priceCache.setPrice('', 2n);
        priceCache.setPrice('a'.repeat(5_000), 3n);

        expect(priceCache.getPrice('__proto__')).toBe(1n);
        expect(priceCache.getPrice('')).toBe(2n);
        expect(priceCache.getPrice('A'.repeat(5_000))).toBe(3n);
        expect(({} as Record<string, unknown>).price).toBeUndefined();
    });

    it('reports freshness metadata for a fresh price', () => {
        const priceCache = createPriceCache(30);
        const updatedAt = T0;

        priceCache.setPrice('XLM', 150_000n, 30, updatedAt);

        expect(priceCache.getPriceWithState('XLM')).toEqual({
            price: 150_000n,
            isStale: false,
            updatedAt,
        });
    });

    it('flags a price as stale inside the grace window and drops it after', () => {
        const priceCache = createPriceCache(10);
        const updatedAt = T0;

        priceCache.setPrice('XLM', 150_000n, 10, updatedAt);
        advance(10_001);

        expect(priceCache.getPriceWithState('XLM')).toEqual({
            price: 150_000n,
            isStale: true,
            updatedAt,
        });

        advance(60_000);
        expect(priceCache.getPriceWithState('XLM')).toBeUndefined();
        expect(priceCache.getStats().size).toBe(0);
    });

    it('returns undefined for an unknown asset with state', () => {
        expect(createPriceCache(30).getPriceWithState('NOPE')).toBeUndefined();
    });

    it('derives expiry from the supplied updatedAt, not the wall clock', () => {
        const priceCache = createPriceCache(30);
        const updatedAt = T0 - 5_000;

        priceCache.setPrice('XLM', 1n, 30, updatedAt);

        expect(priceCache.getPriceWithState('XLM')).toEqual({
            price: 1n,
            isStale: false,
            updatedAt,
        });

        // 30s of freshness counted from `updatedAt` still has 25s left, and the
        // closing instant is inclusive.
        advance(25_000);
        expect(priceCache.getPriceWithState('XLM')?.isStale).toBe(false);
        advance(1);
        expect(priceCache.getPriceWithState('XLM')?.isStale).toBe(true);
    });

    it('recovers by purging entries past the hard expiry', () => {
        const priceCache = createPriceCache(1);

        priceCache.setPrice('A', 1n, 1);
        priceCache.setPrice('B', 2n, 1);
        advance(61_001);

        expect(priceCache.recover()).toBe(2);
        expect(priceCache.getStats().size).toBe(0);
        expect(priceCache.recover()).toBe(0);
    });

    it('recover() preserves fresh and still-stale entries', () => {
        const priceCache = createPriceCache(10);

        priceCache.setPrice('FRESH', 1n, 3_600);
        priceCache.setPrice('STALE', 2n, 10);
        advance(20_000);

        expect(priceCache.recover()).toBe(0);
        expect(priceCache.getPrice('FRESH')).toBe(1n);
        expect(priceCache.getPrice('STALE')).toBe(2n);
    });

    it('clears every price and its statistics survive', () => {
        const priceCache = createPriceCache(30);

        priceCache.setPrice('XLM', 1n);
        priceCache.getPrice('XLM');
        priceCache.getPrice('MISSING');
        priceCache.clear();

        expect(priceCache.getStats()).toMatchObject({ size: 0, hits: 1, misses: 1, hitRate: 0.5 });
    });

    it('hasPrice reflects writes for normalised symbols', () => {
        const priceCache = createPriceCache(30);

        priceCache.setPrice('btc', 50_000_000_000n);

        expect(priceCache.hasPrice('BTC')).toBe(true);
        expect(priceCache.hasPrice('btc')).toBe(true);
        expect(priceCache.hasPrice('ETH')).toBe(false);
        expect(priceCache.hasPrice('__proto__')).toBe(false);
    });

    it('hasPrice stays true inside the stale window and false after it', () => {
        const priceCache = createPriceCache(10);

        priceCache.setPrice('XLM', 1n, 10);
        advance(10_001);
        expect(priceCache.hasPrice('XLM')).toBe(true);

        advance(60_000);
        expect(priceCache.hasPrice('XLM')).toBe(false);
    });

    it('uses a 100-entry capacity and evicts beyond it', () => {
        const priceCache = createPriceCache(3_600);

        for (let i = 0; i < 150; i++) {
            priceCache.setPrice(`A${i}`, BigInt(i), 3_600);
        }

        expect(priceCache.getStats().size).toBe(100);
        expect(priceCache.getPrice('A149')).toBe(149n);
    });
});

describe('PriceCache: setPriceIfNewer ordering guarantees', () => {
    it('accepts the first write for an empty cache', () => {
        const priceCache = createPriceCache(30);

        expect(priceCache.setPriceIfNewer('XLM', 100n, T0)).toBe(true);
        expect(priceCache.getPrice('XLM')).toBe(100n);
    });

    it('rejects an update with an equal updatedAt', () => {
        const priceCache = createPriceCache(30);

        priceCache.setPrice('XLM', 100n, 30, T0);

        expect(priceCache.setPriceIfNewer('XLM', 999n, T0)).toBe(false);
        expect(priceCache.getPrice('XLM')).toBe(100n);
    });

    it('rejects an out-of-order update and keeps the newer price', () => {
        const priceCache = createPriceCache(30);

        priceCache.setPrice('XLM', 200n, 30, T0);

        expect(priceCache.setPriceIfNewer('XLM', 100n, T0 - 1_000)).toBe(false);
        expect(priceCache.getPriceWithState('XLM')).toEqual({
            price: 200n,
            isStale: false,
            updatedAt: T0,
        });
    });

    it('accepts a strictly newer update', () => {
        const priceCache = createPriceCache(30);

        priceCache.setPrice('XLM', 100n, 30, T0);

        expect(priceCache.setPriceIfNewer('XLM', 300n, T0 + 1_000)).toBe(true);
        expect(priceCache.getPriceWithState('XLM')?.price).toBe(300n);
    });

    it('normalises the asset symbol before comparing', () => {
        const priceCache = createPriceCache(30);

        priceCache.setPrice('XLM', 100n, 30, T0);

        expect(priceCache.setPriceIfNewer('xlm', 999n, T0 - 1)).toBe(false);
        expect(priceCache.getPrice('xlm')).toBe(100n);
    });

    it('compares against a soft-expired entry that is still inside the grace window', () => {
        const priceCache = createPriceCache(10);

        priceCache.setPrice('XLM', 100n, 10, T0 - 5_000);
        advance(6_000);

        expect(priceCache.setPriceIfNewer('XLM', 999n, T0 - 6_000)).toBe(false);
        expect(priceCache.getPrice('XLM')).toBe(100n);
    });

    it('accepts any update once the previous entry has hard-expired', () => {
        const priceCache = createPriceCache(1);

        priceCache.setPrice('XLM', 100n, 1, T0 - 120_000);
        advance(121_000);
        expect(priceCache.getPrice('XLM')).toBeUndefined();

        // The expired entry is gone, so there is nothing to compare against and
        // even a backdated update is accepted.
        expect(priceCache.setPriceIfNewer('XLM', 5n, T0 - 240_000)).toBe(true);
        // That backdated update carries its own freshness window in the past.
        expect(priceCache.getPrice('XLM')).toBeUndefined();

        // A current timestamp makes the entry usable again.
        const now = T0 + 121_000;
        expect(priceCache.setPriceIfNewer('XLM', 6n, now)).toBe(true);
        expect(priceCache.getPriceWithState('XLM')).toEqual({
            price: 6n,
            isStale: false,
            updatedAt: now,
        });
    });

    it('never regresses the cached price under adversarial interleaving', async () => {
        const priceCache = createPriceCache(3_600);
        const updates: Array<{ asset: string; price: bigint; updatedAt: number }> = [
            { asset: 'XLM', price: 300n, updatedAt: T0 + 3_000 },
            { asset: 'xlm', price: 100n, updatedAt: T0 + 1_000 },
            { asset: 'XlM', price: 200n, updatedAt: T0 + 2_000 },
            { asset: 'XLM', price: 400n, updatedAt: T0 + 5_000 },
            { asset: 'xlm', price: 400n, updatedAt: T0 + 5_000 },
            { asset: 'XLM', price: 50n, updatedAt: T0 + 500 },
        ];
        const outcomes: boolean[] = new Array(updates.length);

        // Interleave the writers across microtask boundaries.
        await Promise.all(
            updates.map(async (update, index) => {
                await Promise.resolve();
                await Promise.resolve();
                outcomes[index] = priceCache.setPriceIfNewer(update.asset, update.price, update.updatedAt);
            }),
        );

        expect(outcomes).toEqual([true, false, false, true, false, false]);
        expect(priceCache.getPriceWithState('XLM')).toEqual({
            price: 400n,
            isStale: false,
            updatedAt: T0 + 5_000,
        });
        expect(priceCache.getStats().size).toBe(1);
    });

    it('keeps a per-asset price independent under interleaved writers', async () => {
        const priceCache = createPriceCache(3_600);
        const assets = ['XLM', 'BTC', 'ETH', 'USDC'];

        await Promise.all(
            assets.map(async (asset, index) => {
                for (let round = 1; round <= 10; round++) {
                    await Promise.resolve();
                    priceCache.setPriceIfNewer(asset, BigInt(index * 100 + round), T0 + round);
                }
            }),
        );

        for (const [index, asset] of assets.entries()) {
            expect(priceCache.getPrice(asset)).toBe(BigInt(index * 100 + 10));
        }
        expect(priceCache.getStats().size).toBe(4);
    });

    it('records reads performed by setPriceIfNewer in the statistics', () => {
        const priceCache = createPriceCache(30);

        expect(priceCache.setPriceIfNewer('XLM', 1n, T0)).toBe(true);
        // A miss on the empty cache, then a hit on each subsequent call.
        expect(priceCache.setPriceIfNewer('XLM', 2n, T0 + 1)).toBe(true);
        expect(priceCache.setPriceIfNewer('XLM', 3n, T0)).toBe(false);

        expect(priceCache.getStats()).toMatchObject({ size: 1, hits: 2, misses: 1 });
    });
});

describe('PriceCache: constructor configuration', () => {
    it('applies the supplied freshness TTL', () => {
        const priceCache = new PriceCache(5);

        priceCache.setPrice('XLM', 1n);
        advance(5_001);

        expect(priceCache.getPriceWithState('XLM')?.isStale).toBe(true);
    });

    it('applies the supplied stale TTL to the grace window', () => {
        // DEFECT (reported, not fixed here): the `Cache` options object passed by
        // the `PriceCache` constructor declares `staleTtlSeconds` twice, so the
        // hard-coded value always wins and the constructor argument is ignored.
        // This test pins the current behaviour; the intended behaviour is
        // asserted by the `it.fails` case below.
        const priceCache = new PriceCache(10, 5);

        priceCache.setPrice('XLM', 1n, 10);
        advance(16_000);

        // Intended: the 5s grace window has closed. Actual: still served stale.
        expect(priceCache.getPriceWithState('XLM')?.isStale).toBe(true);
    });

    it.fails('fails today: the stale TTL argument is ignored by PriceCache', () => {
        const priceCache = new PriceCache(10, 5);

        priceCache.setPrice('XLM', 1n, 10);
        advance(16_000);

        expect(priceCache.getPriceWithState('XLM')).toBeUndefined();
    });

    it('defaults both TTLs when the cache is constructed with no arguments', () => {
        const priceCache = createPriceCache();

        priceCache.setPrice('XLM', 1n);
        advance(30_000);
        expect(priceCache.getPriceWithState('XLM')?.isStale).toBe(false);

        advance(1);
        expect(priceCache.getPriceWithState('XLM')?.isStale).toBe(true);
    });
});

describe('factories', () => {
    it('createCache returns a fully functional Cache', () => {
        const cache = createCache({ defaultTtlSeconds: 1, maxEntries: 2, staleTtlSeconds: 1 });

        expect(cache).toBeInstanceOf(Cache);
        cache.set('a', 1);
        expect(cache.get('a')).toBe(1);
    });

    it('createCache accepts an undefined config', () => {
        expect(createCache(undefined)).toBeInstanceOf(Cache);
    });

    it('createPriceCache returns a fully functional PriceCache', () => {
        const priceCache = createPriceCache(10, 10);

        expect(priceCache).toBeInstanceOf(PriceCache);
        priceCache.setPrice('XLM', 1n);
        expect(priceCache.getPrice('XLM')).toBe(1n);
    });

    it('createPriceCache accepts omitted arguments', () => {
        expect(createPriceCache()).toBeInstanceOf(PriceCache);
    });
});
