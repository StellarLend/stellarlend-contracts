/**
 * Tests for Cache Service
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { Cache, PriceCache, createCache, createPriceCache } from '../src/services/cache.js';

describe('Cache', () => {
    let cache: Cache;

    beforeEach(() => {
        cache = createCache({
            defaultTtlSeconds: 10,
            maxEntries: 100,
        });
    });

    describe('validation and boundaries', () => {
        it('should throw on invalid defaultTtlSeconds', () => {
            expect(() => createCache({ defaultTtlSeconds: -1 })).toThrow();
            expect(() => createCache({ defaultTtlSeconds: Infinity })).toThrow();
            expect(() => createCache({ defaultTtlSeconds: NaN })).toThrow();
        });

        it('should throw on invalid staleTtlSeconds', () => {
            expect(() => createCache({ staleTtlSeconds: -5 })).toThrow();
        });

        it('should throw on invalid maxEntries', () => {
            expect(() => createCache({ maxEntries: 0 })).toThrow();
            expect(() => createCache({ maxEntries: -10 })).toThrow();
            expect(() => createCache({ maxEntries: 1.5 })).toThrow();
        });

        it('should throw on invalid TTL in set()', () => {
            expect(() => cache.set('key', 'val', -1)).toThrow();
            expect(() => cache.set('key', 'val', NaN)).toThrow();
        });
    });

    describe('get/set', () => {
        it('should store and retrieve values', () => {
            cache.set('key1', 'value1');

            expect(cache.get('key1')).toBe('value1');
        });

        it('should return undefined for missing keys', () => {
            expect(cache.get('nonexistent')).toBeUndefined();
        });

        it('should handle different data types', () => {
            cache.set('string', 'hello');
            cache.set('number', 42);
            cache.set('object', { foo: 'bar' });
            cache.set('array', [1, 2, 3]);
            cache.set('bigint', 12345678901234567890n);

            expect(cache.get('string')).toBe('hello');
            expect(cache.get('number')).toBe(42);
            expect(cache.get('object')).toEqual({ foo: 'bar' });
            expect(cache.get('array')).toEqual([1, 2, 3]);
            expect(cache.get('bigint')).toBe(12345678901234567890n);
        });
    });

    describe('TTL expiration', () => {
        it('should expire entries after TTL', async () => {
            cache = createCache({ defaultTtlSeconds: 0.1 });
            cache.set('temp', 'value');

            expect(cache.get('temp')).toBe('value');

            await new Promise(r => setTimeout(r, 150));

            expect(cache.get('temp')).toBeUndefined();
        });

        it('should use custom TTL when provided', async () => {
            cache.set('custom', 'value', 0.05);

            expect(cache.get('custom')).toBe('value');

            await new Promise(r => setTimeout(r, 100));

            expect(cache.get('custom')).toBeUndefined();
        });
    });

    describe('has', () => {
        it('should return true for existing keys', () => {
            cache.set('exists', 'value');

            expect(cache.has('exists')).toBe(true);
        });

        it('should return false for missing keys', () => {
            expect(cache.has('missing')).toBe(false);
        });

        it('should return false for expired keys', async () => {
            // staleTtlSeconds: 0 disables the stale grace window so the entry
            // is fully expired, rather than only soft-expired.
            cache = createCache({ defaultTtlSeconds: 0.05, staleTtlSeconds: 0 });
            cache.set('expires', 'value');

            await new Promise(r => setTimeout(r, 100));

            expect(cache.has('expires')).toBe(false);
        });
    });

    describe('delete', () => {
        it('should delete existing keys', () => {
            cache.set('toDelete', 'value');

            expect(cache.delete('toDelete')).toBe(true);
            expect(cache.get('toDelete')).toBeUndefined();
        });

        it('should return false for non-existent keys', () => {
            expect(cache.delete('nonexistent')).toBe(false);
        });
    });

    describe('getStale', () => {
        it('should return undefined for missing keys', () => {
            expect(cache.getStale('nonexistent')).toBeUndefined();
        });

        it('should return fresh data with isStale false', () => {
            cache.set('fresh', 'value');
            expect(cache.getStale('fresh')).toEqual({ data: 'value', isStale: false });
        });

        it('should return stale data with isStale true if within grace period', async () => {
            cache = createCache({ defaultTtlSeconds: 0.05, staleTtlSeconds: 0.2 });
            cache.set('stale_data', 'value');

            await new Promise(r => setTimeout(r, 100));

            expect(cache.getStale('stale_data')).toEqual({ data: 'value', isStale: true });
        });

        it('should return undefined and delete if past hard expiry', async () => {
            cache = createCache({ defaultTtlSeconds: 0.05, staleTtlSeconds: 0.05 });
            cache.set('hard_expired', 'value');

            await new Promise(r => setTimeout(r, 150));

            expect(cache.getStale('hard_expired')).toBeUndefined();
            expect(cache.getStats().size).toBe(0);
        });
    });

    describe('clear', () => {
        it('should remove all entries', () => {
            cache.set('key1', 'value1');
            cache.set('key2', 'value2');
            cache.set('key3', 'value3');

            cache.clear();

            expect(cache.get('key1')).toBeUndefined();
            expect(cache.get('key2')).toBeUndefined();
            expect(cache.get('key3')).toBeUndefined();
        });
    });

    describe('stats', () => {
        it('should track hits and misses', () => {
            cache.set('hit', 'value');

            cache.get('hit');
            cache.get('hit');
            cache.get('miss');

            const stats = cache.getStats();

            expect(stats.hits).toBe(2);
            expect(stats.misses).toBe(1);
            expect(stats.hitRate).toBeCloseTo(0.667, 2);
        });

        it('should track size', () => {
            cache.set('a', 1);
            cache.set('b', 2);
            cache.set('c', 3);

            const stats = cache.getStats();

            expect(stats.size).toBe(3);
        });
    });

    describe('eviction', () => {
        it('should evict oldest entry when at capacity', () => {
            cache = createCache({ maxEntries: 3 });

            cache.set('first', 1);
            cache.set('second', 2);
            cache.set('third', 3);
            cache.set('fourth', 4);

            expect(cache.get('first')).toBeUndefined();
            expect(cache.get('second')).toBe(2);
            expect(cache.get('fourth')).toBe(4);
        });
    });

    describe('cleanup', () => {
        it('should remove expired entries', async () => {
            // staleTtlSeconds: 0 so entries cross the hard expiry, not just the
            // freshness TTL, and become eligible for cleanup.
            cache = createCache({ defaultTtlSeconds: 0.05, staleTtlSeconds: 0 });

            cache.set('expire1', 1);
            cache.set('expire2', 2);

            await new Promise(r => setTimeout(r, 100));

            const cleaned = cache.cleanup();

            expect(cleaned).toBe(2);
            expect(cache.getStats().size).toBe(0);
        });

        it('should preserve stale entries during grace period', async () => {
            // defaultTtl: 0.05s, staleTtl: 0.2s -> hard expiry at 0.25s
            cache = createCache({ defaultTtlSeconds: 0.05, staleTtlSeconds: 0.2 });

            cache.set('stale_but_preserved', 1);

            // Wait 100ms: past freshness, but within stale grace period
            await new Promise(r => setTimeout(r, 100));

            const cleaned = cache.cleanup();
            
            // Should not be cleaned up yet
            expect(cleaned).toBe(0);
            expect(cache.getStats().size).toBe(1);

            // Wait another 200ms: past hard expiry (total > 300ms)
            await new Promise(r => setTimeout(r, 200));

            const cleanedAfter = cache.cleanup();
            expect(cleanedAfter).toBe(1);
            expect(cache.getStats().size).toBe(0);
        });
    });
});

describe('PriceCache', () => {
    let priceCache: PriceCache;

    beforeEach(() => {
        priceCache = createPriceCache(30);
    });

    describe('getPriceWithState', () => {
        it('should return undefined if no price', () => {
            expect(priceCache.getPriceWithState('NONEXISTENT')).toBeUndefined();
        });

        it('should return price with isStale false when fresh', () => {
            const now = Date.now();
            priceCache.setPrice('FRESH', 1000n, 10, now);
            const state = priceCache.getPriceWithState('FRESH');
            expect(state).toEqual({
                price: 1000n,
                isStale: false,
                updatedAt: now
            });
        });

        it('should return price with isStale true when stale', async () => {
            const shortCache = createPriceCache(0.05, 0.2);
            const now = Date.now();
            shortCache.setPrice('STALE', 1000n, undefined, now);

            await new Promise(r => setTimeout(r, 100));

            const state = shortCache.getPriceWithState('STALE');
            expect(state).toEqual({
                price: 1000n,
                isStale: true,
                updatedAt: now
            });
        });
    });

    describe('price operations', () => {
        it('should store and retrieve prices as bigint', () => {
            const price = 150000n;

            priceCache.setPrice('XLM', price);

            expect(priceCache.getPrice('XLM')).toBe(price);
        });

        it('should get stale price if within grace period', async () => {
            const shortCache = createPriceCache(0.05, 0.2);
            shortCache.setPrice('STALE', 1000n);
            await new Promise(r => setTimeout(r, 100));
            expect(shortCache.getPrice('STALE')).toBe(1000n);
        });

        it('should return undefined if price is past hard expiry', async () => {
            const shortCache = createPriceCache(0.05, 0.05);
            shortCache.setPrice('EXPIRED', 1000n);
            await new Promise(r => setTimeout(r, 150));
            expect(shortCache.getPrice('EXPIRED')).toBeUndefined();
        });

        it('should normalize asset symbols to uppercase', () => {
            priceCache.setPrice('xlm', 150000n);

            expect(priceCache.getPrice('XLM')).toBe(150000n);
            expect(priceCache.getPrice('xlm')).toBe(150000n);
        });

        it('should check if price exists and include stale but not expired', async () => {
            // Using small TTLs to test expiration
            const shortCache = createPriceCache(0.05, 0.2);
            shortCache.setPrice('BTC', 50000000000n);

            expect(shortCache.hasPrice('BTC')).toBe(true);

            // Wait 100ms - should be stale but within grace period
            await new Promise(r => setTimeout(r, 100));
            expect(shortCache.hasPrice('BTC')).toBe(true);

            // Wait another 200ms - should cross hard expiry
            await new Promise(r => setTimeout(r, 200));
            expect(shortCache.hasPrice('BTC')).toBe(false);
            
            expect(shortCache.hasPrice('ETH')).toBe(false);
        });
    });

    describe('setPriceIfNewer', () => {
        it('should update price if newer', () => {
            const olderTime = Date.now() - 1000;
            priceCache.setPrice('XLM', 150000n, undefined, olderTime);

            const result = priceCache.setPriceIfNewer('XLM', 160000n, Date.now());
            expect(result).toBe(true);
            expect(priceCache.getPrice('XLM')).toBe(160000n);
        });

        it('should not update price if older or equal timestamp', () => {
            const newerTime = Date.now();
            priceCache.setPrice('XLM', 150000n, undefined, newerTime);

            const result = priceCache.setPriceIfNewer('XLM', 140000n, newerTime - 1000);
            expect(result).toBe(false);
            expect(priceCache.getPrice('XLM')).toBe(150000n); // Unchanged
        });
    });

    describe('clear', () => {
        it('should clear all prices', () => {
            priceCache.setPrice('XLM', 150000n);
            priceCache.setPrice('BTC', 50000000000n);

            priceCache.clear();

            expect(priceCache.hasPrice('XLM')).toBe(false);
            expect(priceCache.hasPrice('BTC')).toBe(false);
        });
    });

    describe('recover', () => {
        it('should purge hard-expired entries and return count', async () => {
            const shortCache = createPriceCache(0.05, 0.05); // hard expiry at 0.1s
            shortCache.setPrice('BTC', 100n);
            shortCache.setPrice('ETH', 200n);

            await new Promise(r => setTimeout(r, 150));

            const recoveredCount = shortCache.recover();
            expect(recoveredCount).toBe(2);
            expect(shortCache.hasPrice('BTC')).toBe(false);
        });

        it('should preserve stale entries within grace period during recovery', async () => {
            const shortCache = createPriceCache(0.05, 0.5); // hard expiry at 0.55s
            shortCache.setPrice('BTC', 100n);

            await new Promise(r => setTimeout(r, 100)); // Stale but not hard-expired

            const recoveredCount = shortCache.recover();
            expect(recoveredCount).toBe(0);
            expect(shortCache.hasPrice('BTC')).toBe(true);
        });
    });

    describe('stats', () => {
        it('should return cache statistics', () => {
            priceCache.setPrice('XLM', 150000n);
            priceCache.getPrice('XLM');
            priceCache.getPrice('ETH');

            const stats = priceCache.getStats();

            expect(stats.hits).toBe(1);
            expect(stats.misses).toBe(1);
        });
    });
});
