/**
 * Comprehensive failure-path and boundary coverage for oracle/src/services/index.ts
 * 
 * Tests that all exported services maintain deterministic behavior under:
 * - Invalid inputs
 * - Boundary conditions
 * - Concurrent execution
 * - Partial failures
 * - State transitions
 */

import { describe, it, expect, beforeEach } from 'vitest';
import {
  PriceValidator,
  createValidator,
  Cache,
  PriceCache,
  createCache,
  createPriceCache,
  PriceAggregator,
  createAggregator,
  ContractUpdater,
  createContractUpdater,
} from '../src/services/index.js';
import type {
  ValidatorConfig,
  CacheConfig,
  AggregatorConfig,
  ContractUpdaterConfig,
} from '../src/services/index.js';

describe('Services Index - Export Integrity', () => {
  describe('Export Availability', () => {
    it('should export PriceValidator class', () => {
      expect(PriceValidator).toBeDefined();
      expect(typeof PriceValidator).toBe('function');
    });

    it('should export createValidator factory', () => {
      expect(createValidator).toBeDefined();
      expect(typeof createValidator).toBe('function');
    });

    it('should export Cache class', () => {
      expect(Cache).toBeDefined();
      expect(typeof Cache).toBe('function');
    });

    it('should export PriceCache class', () => {
      expect(PriceCache).toBeDefined();
      expect(typeof PriceCache).toBe('function');
    });

    it('should export createCache factory', () => {
      expect(createCache).toBeDefined();
      expect(typeof createCache).toBe('function');
    });

    it('should export createPriceCache factory', () => {
      expect(createPriceCache).toBeDefined();
      expect(typeof createPriceCache).toBe('function');
    });

    it('should export PriceAggregator class', () => {
      expect(PriceAggregator).toBeDefined();
      expect(typeof PriceAggregator).toBe('function');
    });

    it('should export createAggregator factory', () => {
      expect(createAggregator).toBeDefined();
      expect(typeof createAggregator).toBe('function');
    });

    it('should export ContractUpdater class', () => {
      expect(ContractUpdater).toBeDefined();
      expect(typeof ContractUpdater).toBe('function');
    });

    it('should export createContractUpdater factory', () => {
      expect(createContractUpdater).toBeDefined();
      expect(typeof createContractUpdater).toBe('function');
    });
  });

  describe('Factory Function Behavior', () => {
    it('should create PriceValidator via factory with default config', () => {
      const validator = createValidator();
      expect(validator).toBeInstanceOf(PriceValidator);
    });

    it('should create PriceValidator via factory with custom config', () => {
      const config: Partial<ValidatorConfig> = {
        maxDeviationPercent: 5,
        maxStalenessSeconds: 120,
        minPrice: 0.001,
        maxPrice: 10000,
      };
      const validator = createValidator(config);
      expect(validator).toBeInstanceOf(PriceValidator);
    });

    it('should create Cache via factory with default config', () => {
      const cache = createCache();
      expect(cache).toBeInstanceOf(Cache);
    });

    it('should create Cache via factory with custom config', () => {
      const config: Partial<CacheConfig> = {
        defaultTtlSeconds: 60,
        maxEntries: 500,
        staleTtlSeconds: 120,
      };
      const cache = createCache(config);
      expect(cache).toBeInstanceOf(Cache);
    });

    it('should create PriceCache via factory with default params', () => {
      const priceCache = createPriceCache();
      expect(priceCache).toBeInstanceOf(PriceCache);
    });

    it('should create PriceCache via factory with custom TTL', () => {
      const priceCache = createPriceCache(60, 300);
      expect(priceCache).toBeInstanceOf(PriceCache);
    });

    it('should create ContractUpdater via factory', () => {
      const mockAdapter = {
        submit: async () => ({ txHash: 'test-hash' }),
        getLatestUpdate: async () => null,
      };
      const updater = createContractUpdater(mockAdapter);
      expect(updater).toBeInstanceOf(ContractUpdater);
    });
  });
});

describe('Services Index - Failure Paths and Boundary Conditions', () => {
  describe('PriceValidator - Invalid Inputs', () => {
    it('should handle null config gracefully', () => {
      expect(() => createValidator(null as any)).not.toThrow();
    });

    it('should handle undefined config gracefully', () => {
      expect(() => createValidator(undefined)).not.toThrow();
    });

    it('should reject invalid maxDeviationPercent', () => {
      expect(() =>
        createValidator({ maxDeviationPercent: -5 })
      ).toThrow();
    });

    it('should reject zero maxDeviationPercent', () => {
      expect(() =>
        createValidator({ maxDeviationPercent: 0 })
      ).toThrow();
    });

    it('should reject invalid maxStalenessSeconds', () => {
      expect(() =>
        createValidator({ maxStalenessSeconds: -10 })
      ).toThrow();
    });

    it('should reject NaN maxStalenessSeconds', () => {
      expect(() =>
        createValidator({ maxStalenessSeconds: NaN })
      ).toThrow();
    });

    it('should reject Infinity maxStalenessSeconds', () => {
      expect(() =>
        createValidator({ maxStalenessSeconds: Infinity })
      ).toThrow();
    });

    it('should reject invalid minPrice', () => {
      expect(() =>
        createValidator({ minPrice: -0.01 })
      ).toThrow();
    });

    it('should reject zero minPrice', () => {
      expect(() =>
        createValidator({ minPrice: 0 })
      ).toThrow();
    });

    it('should reject maxPrice less than minPrice', () => {
      expect(() =>
        createValidator({ minPrice: 100, maxPrice: 50 })
      ).toThrow();
    });

    it('should reject invalid asset bounds', () => {
      expect(() =>
        createValidator({}, { BTC: { minPrice: -1, maxPrice: 100000 } })
      ).toThrow();
    });

    it('should reject asset bounds with maxPrice < minPrice', () => {
      expect(() =>
        createValidator({}, { BTC: { minPrice: 50000, maxPrice: 10000 } })
      ).toThrow();
    });
  });

  describe('Cache - Boundary Conditions', () => {
    it('should handle zero TTL', () => {
      const cache = createCache({ defaultTtlSeconds: 0 });
      cache.set('test', 'value');
      expect(cache.get('test')).toBeUndefined();
    });

    it('should reject negative TTL', () => {
      expect(() =>
        createCache({ defaultTtlSeconds: -10 })
      ).toThrow();
    });

    it('should reject negative staleTtlSeconds', () => {
      expect(() =>
        createCache({ staleTtlSeconds: -5 })
      ).toThrow();
    });

    it('should handle maxEntries boundary', () => {
      const cache = createCache({ maxEntries: 2 });
      cache.set('key1', 'value1');
      cache.set('key2', 'value2');
      cache.set('key3', 'value3');
      expect(cache.getStats().size).toBe(2);
    });

    it('should handle empty key', () => {
      const cache = createCache();
      cache.set('', 'value');
      expect(cache.get('')).toBe('value');
    });

    it('should handle very long keys', () => {
      const cache = createCache();
      const longKey = 'x'.repeat(10000);
      cache.set(longKey, 'value');
      expect(cache.get(longKey)).toBe('value');
    });

    it('should handle special character keys', () => {
      const cache = createCache();
      const specialKey = '!@#$%^&*(){}[]|\\:";\'<>?,./';
      cache.set(specialKey, 'value');
      expect(cache.get(specialKey)).toBe('value');
    });

    it('should handle null values', () => {
      const cache = createCache();
      cache.set('test', null as any);
      expect(cache.get('test')).toBe(null);
    });

    it('should handle undefined values', () => {
      const cache = createCache();
      cache.set('test', undefined as any);
      expect(cache.get('test')).toBe(undefined);
    });
  });

  describe('PriceCache - Boundary Conditions', () => {
    it('should handle zero TTL', () => {
      const priceCache = createPriceCache(0, 60);
      priceCache.setPrice('BTC', 50000n);
      expect(priceCache.getPrice('BTC')).toBeUndefined();
    });

    it('should handle very large prices', () => {
      const priceCache = createPriceCache(60);
      const largePrice = BigInt('999999999999999999');
      priceCache.setPrice('BTC', largePrice);
      expect(priceCache.getPrice('BTC')).toBe(largePrice);
    });

    it('should handle zero price', () => {
      const priceCache = createPriceCache(60);
      priceCache.setPrice('BTC', 0n);
      expect(priceCache.getPrice('BTC')).toBe(0n);
    });

    it('should normalize asset names to uppercase', () => {
      const priceCache = createPriceCache(60);
      priceCache.setPrice('btc', 50000n);
      expect(priceCache.getPrice('BTC')).toBe(50000n);
      expect(priceCache.getPrice('btc')).toBe(50000n);
    });

    it('should handle missing asset gracefully', () => {
      const priceCache = createPriceCache(60);
      expect(priceCache.getPrice('UNKNOWN')).toBeUndefined();
    });

    it('should handle empty asset name', () => {
      const priceCache = createPriceCache(60);
      priceCache.setPrice('', 1000n);
      expect(priceCache.getPrice('')).toBe(1000n);
    });
  });

  describe('ContractUpdater - Invalid Inputs', () => {
    it('should handle null adapter', () => {
      expect(() => createContractUpdater(null as any)).not.toThrow();
    });

    it('should handle undefined adapter', () => {
      expect(() => createContractUpdater(undefined as any)).not.toThrow();
    });

    it('should handle adapter without submit method', () => {
      const badAdapter = { getLatestUpdate: async () => null };
      const updater = createContractUpdater(badAdapter as any);
      expect(updater).toBeInstanceOf(ContractUpdater);
    });

    it('should handle adapter without getLatestUpdate method', () => {
      const badAdapter = { submit: async () => ({ txHash: 'test' }) };
      const updater = createContractUpdater(badAdapter as any);
      expect(updater).toBeInstanceOf(ContractUpdater);
    });
  });
});

describe('Services Index - Concurrent Execution Safety', () => {
  describe('Cache - Concurrent Access', () => {
    it('should handle concurrent writes to same key', () => {
      const cache = createCache();
      const promises = Array.from({ length: 100 }, (_, i) =>
        Promise.resolve(cache.set('test', `value${i}`))
      );

      return Promise.all(promises).then(() => {
        const value = cache.get('test');
        expect(value).toMatch(/value\d+/);
      });
    });

    it('should handle concurrent reads and writes', async () => {
      const cache = createCache();
      cache.set('test', 'initial');

      const operations = Array.from({ length: 50 }, (_, i) => {
        if (i % 2 === 0) {
          return Promise.resolve(cache.get('test'));
        } else {
          return Promise.resolve(cache.set('test', `value${i}`));
        }
      });

      await Promise.all(operations);
      expect(cache.has('test')).toBe(true);
    });

    it('should handle concurrent deletes', () => {
      const cache = createCache();
      cache.set('test', 'value');

      const deletes = Array.from({ length: 10 }, () =>
        Promise.resolve(cache.delete('test'))
      );

      return Promise.all(deletes).then(() => {
        expect(cache.has('test')).toBe(false);
      });
    });
  });

  describe('PriceCache - Concurrent Price Updates', () => {
    it('should handle concurrent price updates for same asset', () => {
      const priceCache = createPriceCache(60);

      const updates = Array.from({ length: 100 }, (_, i) =>
        Promise.resolve(priceCache.setPrice('BTC', BigInt(50000 + i)))
      );

      return Promise.all(updates).then(() => {
        const price = priceCache.getPrice('BTC');
        expect(price).toBeDefined();
        expect(price).toBeGreaterThanOrEqual(50000n);
      });
    });

    it('should handle concurrent updates for different assets', () => {
      const priceCache = createPriceCache(60);

      const assets = ['BTC', 'ETH', 'XLM', 'USDC'];
      const updates = assets.flatMap((asset, i) =>
        Array.from({ length: 25 }, () =>
          Promise.resolve(priceCache.setPrice(asset, BigInt(1000 * (i + 1))))
        )
      );

      return Promise.all(updates).then(() => {
        assets.forEach((asset) => {
          expect(priceCache.getPrice(asset)).toBeDefined();
        });
      });
    });
  });

  describe('PriceValidator - Concurrent Validations', () => {
    it('should handle concurrent validations of same asset', () => {
      const validator = createValidator();
      const now = Math.floor(Date.now() / 1000);

      const validations = Array.from({ length: 50 }, (_, i) =>
        validator.validate({
          asset: 'BTC',
          price: 50000 + i,
          timestamp: now - i,
          source: 'test',
        })
      );

      validations.forEach((result) => {
        expect(result).toHaveProperty('isValid');
        expect(result).toHaveProperty('errors');
      });
    });

    it('should handle concurrent validations of different assets', () => {
      const validator = createValidator();
      const now = Math.floor(Date.now() / 1000);
      const assets = ['BTC', 'ETH', 'XLM', 'USDC', 'SOL'];

      const validations = assets.flatMap((asset, i) =>
        Array.from({ length: 20 }, () =>
          validator.validate({
            asset,
            price: 1000 + i * 100,
            timestamp: now,
            source: 'test',
          })
        )
      );

      validations.forEach((result) => {
        expect(result).toHaveProperty('isValid');
      });
    });
  });
});

describe('Services Index - State Transition Invariants', () => {
  describe('PriceValidator - State Consistency', () => {
    it('should maintain cache consistency after validation', () => {
      const validator = createValidator();
      const now = Math.floor(Date.now() / 1000);

      const result = validator.validate({
        asset: 'BTC',
        price: 50000,
        timestamp: now,
        source: 'test',
      });

      expect(result.isValid).toBe(true);
      const cacheState = validator.getCacheState();
      expect(cacheState).toBeDefined();
    });

    it('should clear pending state on commit', () => {
      const validator = createValidator();
      const now = Math.floor(Date.now() / 1000);

      validator.validate({
        asset: 'BTC',
        price: 50000,
        timestamp: now,
        source: 'test',
      });

      validator.commit('BTC');
      const cacheState = validator.getCacheState();
      expect(cacheState.BTC).toBe(50000);
    });

    it('should clear pending state on rollback', () => {
      const validator = createValidator();
      const now = Math.floor(Date.now() / 1000);

      validator.validate({
        asset: 'BTC',
        price: 50000,
        timestamp: now,
        source: 'test',
      });

      validator.rollback('BTC');
      const cacheState = validator.getCacheState();
      expect(cacheState.BTC).toBeUndefined();
    });

    it('should handle commit without pending validation', () => {
      const validator = createValidator();
      expect(() => validator.commit('BTC')).not.toThrow();
    });

    it('should handle rollback without pending validation', () => {
      const validator = createValidator();
      expect(() => validator.rollback('BTC')).not.toThrow();
    });

    it('should prevent duplicate validations at same timestamp', () => {
      const validator = createValidator();
      const now = Math.floor(Date.now() / 1000);

      const rawPrice = {
        asset: 'BTC',
        price: 50000,
        timestamp: now,
        source: 'test',
      };

      const result1 = validator.validate(rawPrice);
      expect(result1.isValid).toBe(true);

      const result2 = validator.validate(rawPrice);
      expect(result2.isValid).toBe(false);
      expect(result2.errors[0].code).toBe('PRICE_STALE');
    });
  });

  describe('Cache - Expiration Invariants', () => {
    it('should never return expired data via get', () => {
      const cache = createCache({ defaultTtlSeconds: 0.1 });
      cache.set('test', 'value');

      return new Promise((resolve) => {
        setTimeout(() => {
          expect(cache.get('test')).toBeUndefined();
          resolve(undefined);
        }, 150);
      });
    });

    it('should maintain size limits under load', () => {
      const cache = createCache({ maxEntries: 10 });

      for (let i = 0; i < 100; i++) {
        cache.set(`key${i}`, `value${i}`);
      }

      expect(cache.getStats().size).toBeLessThanOrEqual(10);
    });

    it('should cleanup stale entries correctly', async () => {
      const cache = createCache({
        defaultTtlSeconds: 0.1,
        staleTtlSeconds: 0.2,
      });

      cache.set('test1', 'value1');
      cache.set('test2', 'value2');

      await new Promise((resolve) => setTimeout(resolve, 400));

      const cleaned = cache.cleanup();
      expect(cleaned).toBeGreaterThan(0);
    });
  });

  describe('PriceCache - Timestamp Monotonicity', () => {
    it('should reject older timestamps', () => {
      const priceCache = createPriceCache(60);
      const now = Date.now();

      priceCache.setPrice('BTC', 50000n, undefined, now);
      priceCache.setPrice('BTC', 51000n, undefined, now - 1000);

      expect(priceCache.getPrice('BTC')).toBe(50000n);
    });

    it('should accept newer timestamps', () => {
      const priceCache = createPriceCache(60);
      const now = Date.now();

      priceCache.setPrice('BTC', 50000n, undefined, now);
      priceCache.setPrice('BTC', 51000n, undefined, now + 1000);

      expect(priceCache.getPrice('BTC')).toBe(51000n);
    });

    it('should handle setPriceIfNewer correctly', () => {
      const priceCache = createPriceCache(60);
      const now = Date.now();

      const result1 = priceCache.setPriceIfNewer('BTC', 50000n, now);
      expect(result1).toBe(true);

      const result2 = priceCache.setPriceIfNewer('BTC', 51000n, now - 1000);
      expect(result2).toBe(false);

      const result3 = priceCache.setPriceIfNewer('BTC', 52000n, now + 1000);
      expect(result3).toBe(true);
    });
  });
});

describe('Services Index - Error Recovery and Resilience', () => {
  describe('Cache - Recovery from Corruption', () => {
    it('should recover from invalid cache entries', () => {
      const cache = createCache();
      cache.set('valid', 'value');

      // Simulate corruption by setting invalid internal state
      (cache as any).store.set('corrupt', null);

      expect(cache.get('valid')).toBe('value');
      expect(() => cache.cleanup()).not.toThrow();
    });

    it('should handle clear after corruption', () => {
      const cache = createCache();
      cache.set('test', 'value');
      (cache as any).store.set('corrupt', { invalid: true });

      expect(() => cache.clear()).not.toThrow();
      expect(cache.getStats().size).toBe(0);
    });
  });

  describe('PriceValidator - Graceful Degradation', () => {
    it('should continue working after validation failures', () => {
      const validator = createValidator();
      const now = Math.floor(Date.now() / 1000);

      validator.validate({
        asset: 'BTC',
        price: -1,
        timestamp: now,
        source: 'test',
      });

      const result = validator.validate({
        asset: 'ETH',
        price: 3000,
        timestamp: now,
        source: 'test',
      });

      expect(result.isValid).toBe(true);
    });

    it('should handle config reload without disrupting state', () => {
      const validator = createValidator();
      const now = Math.floor(Date.now() / 1000);

      validator.validate({
        asset: 'BTC',
        price: 50000,
        timestamp: now,
        source: 'test',
      });

      validator.reloadConfig({ maxDeviationPercent: 20 });

      const cacheState = validator.getCacheState();
      expect(cacheState).toBeDefined();
    });

    it('should clear cache selectively', () => {
      const validator = createValidator();
      const now = Math.floor(Date.now() / 1000);

      validator.validate({
        asset: 'BTC',
        price: 50000,
        timestamp: now,
        source: 'test',
      });
      validator.commit('BTC');

      validator.validate({
        asset: 'ETH',
        price: 3000,
        timestamp: now,
        source: 'test',
      });
      validator.commit('ETH');

      validator.clearCache('BTC');
      const state = validator.getCacheState();

      expect(state.BTC).toBeUndefined();
      expect(state.ETH).toBeDefined();
    });

    it('should clear all cache when no asset specified', () => {
      const validator = createValidator();
      const now = Math.floor(Date.now() / 1000);

      validator.validate({
        asset: 'BTC',
        price: 50000,
        timestamp: now,
        source: 'test',
      });
      validator.commit('BTC');

      validator.clearCache();
      const state = validator.getCacheState();

      expect(Object.keys(state).length).toBe(0);
    });
  });
});

describe('Services Index - Regression Prevention', () => {
  it('should maintain backward compatibility for all exports', () => {
    const exports = [
      'PriceValidator',
      'createValidator',
      'Cache',
      'PriceCache',
      'createCache',
      'createPriceCache',
      'PriceAggregator',
      'createAggregator',
      'ContractUpdater',
      'createContractUpdater',
    ];

    const serviceIndex = require('../src/services/index.js');

    exports.forEach((exportName) => {
      expect(serviceIndex[exportName]).toBeDefined();
    });
  });

  it('should preserve factory function signatures', () => {
    expect(createValidator.length).toBeLessThanOrEqual(4);
    expect(createCache.length).toBeLessThanOrEqual(1);
    expect(createPriceCache.length).toBeLessThanOrEqual(2);
    expect(createContractUpdater.length).toBeLessThanOrEqual(1);
  });

  it('should preserve class constructors', () => {
    expect(() => new PriceValidator()).not.toThrow();
    expect(() => new Cache()).not.toThrow();
    expect(() => new PriceCache()).not.toThrow();
  });
});

describe('Services Index - Observability and Diagnostics', () => {
  describe('Cache Statistics', () => {
    it('should track hit rate correctly', () => {
      const cache = createCache();
      cache.set('test', 'value');

      cache.get('test'); // hit
      cache.get('missing'); // miss

      const stats = cache.getStats();
      expect(stats.hits).toBe(1);
      expect(stats.misses).toBe(1);
      expect(stats.hitRate).toBe(0.5);
    });

    it('should track cache size', () => {
      const cache = createCache();
      cache.set('key1', 'value1');
      cache.set('key2', 'value2');

      const stats = cache.getStats();
      expect(stats.size).toBe(2);
    });

    it('should handle zero accesses for hitRate', () => {
      const cache = createCache();
      const stats = cache.getStats();
      expect(stats.hitRate).toBe(0);
    });
  });

  describe('PriceCache Statistics', () => {
    it('should expose underlying cache stats', () => {
      const priceCache = createPriceCache(60);
      priceCache.setPrice('BTC', 50000n);

      const stats = priceCache.getStats();
      expect(stats).toHaveProperty('size');
      expect(stats).toHaveProperty('hits');
      expect(stats).toHaveProperty('misses');
    });
  });

  describe('PriceValidator Cache State', () => {
    it('should expose current cache state', () => {
      const validator = createValidator();
      const now = Math.floor(Date.now() / 1000);

      validator.validate({
        asset: 'BTC',
        price: 50000,
        timestamp: now,
        source: 'test',
      });
      validator.commit('BTC');

      const state = validator.getCacheState();
      expect(state).toEqual({ BTC: 50000 });
    });

    it('should return empty object when cache is empty', () => {
      const validator = createValidator();
      const state = validator.getCacheState();
      expect(state).toEqual({});
    });
  });
});
