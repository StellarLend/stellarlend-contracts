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
            cache = createCache({ defaultTtlSeconds: 0.05 });
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
            cache = createCache({ defaultTtlSeconds: 0.05 });

            cache.set('expire1', 1);
            cache.set('expire2', 2);

            await new Promise(r => setTimeout(r, 100));

            const cleaned = cache.cleanup();

            expect(cleaned).toBe(2);
            expect(cache.getStats().size).toBe(0);
        });
    });
});

describe('PriceCache', () => {
    let priceCache: PriceCache;

    beforeEach(() => {
        priceCache = createPriceCache(30);
    });

    describe('price operations', () => {
        it('should store and retrieve prices as bigint', () => {
            const price = 150000n;

            priceCache.setPrice('XLM', price);

            expect(priceCache.getPrice('XLM')).toBe(price);
        });

        it('should normalize asset symbols to uppercase', () => {
            priceCache.setPrice('xlm', 150000n);

            expect(priceCache.getPrice('XLM')).toBe(150000n);
            expect(priceCache.getPrice('xlm')).toBe(150000n);
        });

        it('should check if price exists', () => {
            priceCache.setPrice('BTC', 50000000000n);

            expect(priceCache.hasPrice('BTC')).toBe(true);
            expect(priceCache.hasPrice('ETH')).toBe(false);
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
