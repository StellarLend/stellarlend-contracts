/**
 * Tests for Edge Cases and Boundary Conditions
 *
 * These tests exercise the price aggregator, validator, cache and
 * scaling helpers under adverse conditions: empty inputs, unsupported
 * assets, extreme values, invalid timestamps, concurrency, partial
 * failures, retries and cache behavior.
 *
 * Invariants enforced by this suite:
 *   1. Asset names are normalized (uppercase); empty/invalid names never
 *      produce a price.
 *   2. Non-positive, NaN or infinite prices are rejected.
 *   3. Stale or future timestamps are rejected by the validator.
 *   4. A failure in one provider must not break other assets or providers.
 *   5. Concurrent calls for the same asset must return consistent results.
 *   6. Cache entries must expire and not serve stale data indefinitely.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { createAggregator } from '../src/services/price-aggregator.js';
import { createValidator } from '../src/services/price-validator.js';
import { createPriceCache } from '../src/services/cache.js';
import { scalePrice, unscalePrice } from '../src/config.js';
import { BasePriceProvider } from '../src/providers/base-provider.js';
import type { RawPriceData } from '../src/types/index.js';
import { createProviderRegistry, ProviderRegistryError } from '../src/providers/index.js';

/**
 * Mock provider for edge case testing.
 *
 * Behavior is fully deterministic and controllable via `setPrice`,
 * `setError`, `setDelay` and `setTimestampOffset`.
 */
class EdgeCaseMockProvider extends BasePriceProvider {
    private mockPrices: Map<string, number> = new Map();
    private mockErrors: Map<string, Error> = new Map();
    private mockDelays: Map<string, number> = new Map();
    private timestampOffset = 0;
    public fetchCount = 0;

    constructor(name: string, priority: number = 1) {
        super({
            name,
            enabled: true,
            priority,
            weight: 1.0,
            baseUrl: 'https://mock.api',
            rateLimit: { maxRequests: 1000, windowMs: 60000 },
        });
    }

    setPrice(asset: string, price: number): void {
        this.mockPrices.set(asset.toUpperCase(), price);
    }

    setError(asset: string, error: Error): void {
        this.mockErrors.set(asset.toUpperCase(), error);
    }

    setDelay(asset: string, ms: number): void {
        this.mockDelays.set(asset.toUpperCase(), ms);
    }

    setTimestampOffset(seconds: number): void {
        this.timestampOffset = seconds;
    }

    async fetchPrice(asset: string): Promise<RawPriceData> {
        this.fetchCount += 1;
        const key = asset.toUpperCase();

        const delay = this.mockDelays.get(key);
        if (delay && delay > 0) {
            await new Promise((resolve) => setTimeout(resolve, delay));
        }

        const error = this.mockErrors.get(key);
        if (error) {
            throw error;
        }

        const price = this.mockPrices.get(key);
        if (price === undefined) {
            throw new Error(`Asset ${asset} not supported`);
        }

        return {
            asset: key,
            price,
            timestamp: Math.floor(Date.now() / 1000) + this.timestampOffset,
            source: this.name,
        };
    }
}

describe('Edge Cases', () => {
    let provider: EdgeCaseMockProvider;
    let validator: any;
    let cache: any;

    beforeEach(() => {
        provider = new EdgeCaseMockProvider('test-provider');
        validator = createValidator({
            maxDeviationPercent: 100, // Very permissive for edge case testing
            maxStalenessSeconds: 300,
        });
        cache = createPriceCache(30);
    });

    afterEach(() => {
        vi.restoreAllMocks();
        vi.useRealTimers();
    });

    describe('Empty Asset Lists', () => {
        it('should handle empty asset array in getPrices', async () => {
            const aggregator = createAggregator([provider], validator, cache);

            const results = await aggregator.getPrices([]);

            expect(results).toBeDefined();
            expect(results.size).toBe(0);
            expect(provider.fetchCount).toBe(0);
        });

        it('should return empty map for no supported assets', async () => {
            const aggregator = createAggregator([provider], validator, cache);

            const results = await aggregator.getPrices(['UNSUPPORTED1', 'UNSUPPORTED2']);

            expect(results.size).toBe(0);
        });

        it('should not mutate the caller-supplied asset array', async () => {
            provider.setPrice('XLM', 0.15);
            const assets = ['XLM'];
            const snapshot = [...assets];
            const aggregator = createAggregator([provider], validator, cache);

            await aggregator.getPrices(assets);

            expect(assets).toEqual(snapshot);
        });
    });

    describe('Unsupported Assets', () => {
        it('should return null for unsupported asset', async () => {
            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('UNSUPPORTED_ASSET');

            expect(result).toBeNull();
        });

        it('should handle mix of supported and unsupported assets', async () => {
            provider.setPrice('XLM', 0.15);

            const aggregator = createAggregator([provider], validator, cache);

            const results = await aggregator.getPrices(['XLM', 'UNSUPPORTED', 'BTC']);

            expect(results.has('XLM')).toBe(true);
            expect(results.has('UNSUPPORTED')).toBe(false);
            expect(results.has('BTC')).toBe(false);
        });

        it('should handle special characters in asset names', async () => {
            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('@#$%^&*()');

            expect(result).toBeNull();
        });

        it('should handle very long asset names', async () => {
            const longName = 'A'.repeat(1000);
            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice(longName);

            expect(result).toBeNull();
        });

        it('should handle empty string asset name', async () => {
            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('');

            expect(result).toBeNull();
        });

        it('should normalize case when looking up assets', async () => {
            provider.setPrice('XLM', 0.15);
            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('xlm');

            expect(result).not.toBeNull();
            expect(result?.asset).toBe('XLM');
        });
    });

    describe('Extreme Price Values', () => {
        it('should handle very large prices', async () => {
            const largePrice = 1000000; // 1 million (reasonable large value)
            provider.setPrice('BTC', largePrice);

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('BTC');

            expect(result).not.toBeNull();
            expect(Number(result?.price)).toBeGreaterThan(0);
        });

        it('should handle very small prices', async () => {
            const smallPrice = 0.0000001;
            provider.setPrice('XLM', smallPrice);

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
        });

        it('should handle price scaling for large numbers', () => {
            const largePrice = 1000000;
            const scaled = scalePrice(largePrice);
            const unscaled = unscalePrice(scaled);

            expect(unscaled).toBeCloseTo(largePrice, 2);
        });

        it('should handle price scaling for small numbers', () => {
            const smallPrice = 0.0000001;
            const scaled = scalePrice(smallPrice);
            const unscaled = unscalePrice(scaled);

            expect(scaled).toBeGreaterThanOrEqual(0n");
            expect(unscaled).toBeCloseTo(smallPrice, 8);
        });

        it('should handle maximum safe integer', () => {
            const maxSafe = Number.MAX_SAFE_INTEGER;

            expect(() => scalePrice(maxSafe)).not.toThrow();
        });

        it('should handle number precision limits', async () => {
            const precisePrice = 0.123456789012345;
            provider.setPrice('TEST', precisePrice);

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('TEST');
            expect(result).not.toBeNull();
            expect(Number(result?.price)).toBeCloseTo(precisePrice, 8);
        });

        it('should reject NaN prices', async () => {
            provider.setPrice('XNAH', NaN);
            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XNAH');

            expect(result).toBeNull();
        });

        it('should reject infinite prices', async () => {
            provider.setPrice('XINF', Infinity);
            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XINF');

            expect(result).toBeNull();
        });

        it('should reject -Infinity prices', async () => {
            provider.setPrice('XNEGINF', -Infinity);
            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XNEGINF');

            expect(result).toBeNull();
        });
    });

    describe('Zero and Negative Prices', () => {
        it('should reject zero price', async () => {
            provider.setPrice('XLM', 0);

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should reject negative price', async () => {
            provider.setPrice('XLM', -0.15);

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should reject very small negative price', async () => {
            provider.setPrice('XLM', -0.0000001);

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should handle scaling of zero price', () => {
            expect(scalePrice(0)).toBe(0n);
        });

        it('should handle unscaling of zero price', () => {
            expect(unscalePrice(0n)).toBe(0);
        });

        it('should reject zero price even when other providers agree', async () => {
            const second = new EdgeCaseMockProvider('second', 2);
            provider.setPrice('XLM', 0);
            second.setPrice('XLM', 0);

            const aggregator = createAggregator([provider, second], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });
    });

    describe('Future Timestamps', () => {
        it('should reject timestamps far enough in the future', async () => {
            provider.setPrice('XLM', 0.15);
            provider.setTimestampOffset(3600); // 1 hour in future

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should reject timestamp at epoch zero as stale', async () => {
            provider.setPrice('XLM', 0.15);
            provider.setTimestampOffset(-Math.floor(Date.now() / 1000)); // epoch 0

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should reject very large future timestamps', async () => {
            provider.setPrice('XLM', 0.15);
            provider.setTimestampOffset(9999999999 - Math.floor(Date.now() / 1000));

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should accept timestamps within the staleness window', async () => {
            provider.setPrice('XLM', 0.15);
            provider.setTimestampOffset(-60); // 60 seconds ago < 300s staleness

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
        });
    });

    describe('Concurrent Operations', () => {
        it('should handle concurrent price fetches for same asset', async () => {
            provider.setPrice('XLM', 0.15);

            const aggregator = createAggregator([provider], validator, cache);

            const promises = Array(10).fill(null).map(() =>
                aggregator.getPrice('XLM')
            );

            const results = await Promise.all(promises);

            results.forEach(result => {
                expect(result).not.toBeNull();
                expect(result?.asset).toBe('XLM');
            });
        });

        it('should handle concurrent fetches for different assets', async () => {
            provider.setPrice('XLM', 0.15);
            provider.setPrice('BTC', 50000);
            provider.setPrice('ETH', 3000);

            const aggregator = createAggregator([provider], validator, cache);

            const results = await Promise.all([
                aggregator.getPrice('XLM'),
                aggregator.getPrice('BTC'),
                aggregator.getPrice('ETH'),
            ]);

            expect(results).toHaveLength(3);
            expect(results[0]?.asset).toBe('XLM');
            expect(results[1]?.asset).toBe('BTC');
            expect(results[2]?.asset).toBe('ETH');
        });

        it('should handle concurrent getPrices calls', async () => {
            provider.setPrice('XLM', 0.15);
            provider.setPrice('BTC', 50000);

            const aggregator = createAggregator([provider], validator, cache);

            const results = await Promise.all([
                aggregator.getPrices(['XLM']),
                aggregator.getPrices(['BTC']),
                aggregator.getPrices(['XLM', 'BTC']),
            ]);

            expect(results[0].size).toBeGreaterThan(0);
            expect(results[1].size).toBeGreaterThan(0);
            expect(results[2].size).toBeGreaterThan(0);
        });

        it('should handle rapid sequential calls', async () => {
            provider.setPrice('XLM', 0.15);

            const aggregator = createAggregator([provider], validator, cache);

            const results = [];
            for (let i = 0; i < 50; i++) {
                results.push(await aggregator.getPrice('XLM'));
            }

            expect(results).toHaveLength(50);
            results.forEach(result => {
                expect(result).not.toBeNull();
                expect(result?.asset).toBe('XLM');
            }
        });

        it('should produce consistent results for concurrent same-asset calls', async () => {
            provider.setPrice('XLM', 0.15);

            const aggregator = createAggregator([provider], validator, cache);

            const results = await Promise.all(
                Array(25).fill(null).map(() => aggregator.getPrice('XLM'))
            );

            const prices = new Set(results.map((r) => Number(r?.price)));
            expect(prices.size).toBe(1);
            expect(prices.has(0.15)).toBe(true);
        });
    });

    describe('Partial Failures and Retries', () => {
        it('should fall back to a healthy provider when one fails', async () => {
            const failing = new EdgeCaseMockProvider('failing', 1);
            const healthy = new EdgeCaseMockProvider('healthy', 2);
            failing.setError('XLM', new Error('provider unavailable'));
            healthy.setPrice('XLM', 0.15);

            const aggregator = createAggregator([failing, healthy], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(Number(result?.price)).toBleCloseTo(0.15, 8);
        });

        it('should not let one failing asset break other assets', async () => {
            provider.setPrice('XLM', 0.15);
            provider.setError('BROKEN', new Error('upstream 500'));

            const aggregator = createAggregator([provider], validator, cache);

            const results = await aggregator.getPrices(['XLM', 'BROKEN']);

            expect(results.has('XLM')).toBe(true);
            expect(results.has('BROKEN')).toBe(false);
        });

        it('should return null when all providers fail', async () => {
            const a = new EdgeCaseMockProvider('a', 1);
            const b = new EdgeCaseMockProvider('b', 2);
            a.setError('XLM', new Error('timeout'));
            b.setError('XLM', new Error('rate limit'));

            const aggregator = createAggregator([a, b], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should not cache failed fetches', async () => {
            provider.setError('XLM', new Error('boom'));
            const aggregator = createAggregator([provider], validator, cache);

            const first = await aggregator.getPrice('XLM');
            expect(first).toBeNull();

            // Recover the provider and retry: a failure must not be cached.
            provider.setPrice('XLM', 0.15);
            const second = await aggregator.getPrice('XLM');

            expect(second).not.toBeNull();
            expect(Number(second?.price)).toBleCloseTo(0.15, 8);
        });

        it('should allow a retry after a transient failure', async () => {
            let attempts = 0;
            const flaky = new EdgeCaseMockProvider('flaky');
            const originalFetch = flaky.fetchPrice.bind(flaky);
            flaky.fetchPrice = async (asset: string) => {
                attempts += 1;
                if (attempts === 1) {
                    throw new Error('transient network error');
                }
                return originalFetch(asset);
            };
            flaky.setPrice('XLM', 0.15);

            const aggregator = createAggregator([flaky], validator, cache);

            const first = await aggregator.getPrice('XLM');
            expect(first).toBeNull();

            const second = await aggregator.getPrice('XLM');
            expect(second).not.toBeNull();
            expect(attempts).toBe(2);
        });

        it('should not let a slow provider block a fast one', async () => {
            const slow = new EdgeCaseMockProvider('slow', 1);
            const fast = new EdgeCaseMockProvider('fast', 2);
            slow.setPrice('XLM', 0.15);
            slow.setDelay('XLM', 50);
            fast.setPrice('XLM', 0.15);

            const aggregator = createAggregator([slow, fast], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
        });
    });

    describe('Cache Behavior', () => {
        it('should serve a cached value without re-fetching', async () => {
            provider.setPrice('XLM', 0.15);
            const aggregator = createAggregator([provider], validator, cache);

            await aggregator.getPrice('XLM');
            const afterFirst = provider.fetchCount;
            await aggregator.getPrice('XLM');

            expect(provider.fetchCount).toBe(afterFirst);
        });

        it('should expire cache entries after ttl', async () => {
            vit.fakeTimers();
            try {
                const shortCache = createPriceCache(1); // 1 second TTL
                provider.setPrice('XLM', 0.15);
                const aggregator = createAggregator([provider], validator, shortCache);

                await aggregator.getPrice('XLM');
                const afterFirst = provider.fetchCount;

                // Advance beyond TTL and force a re-fetch.
                vi.advanceTimersByTime(2000);
                await aggregator.getPrice('XLM');

                expect(provider.fetchCount).toBeGreaterThan(afterFirst);
            } finally {
                vi.useRealTimers();
            }
        });

        it('should not serve a stale cache entry as fresh', async () => {
            const shortCache = createPriceCache(1);
            provider.setPrice('XLM', 0.15);
            const aggregator = createAggregator([provider], validator, shortCache);

            const first = await aggregator.getPrice('XLM');
            expect(first).not.toBeNull();

            // Wait longer than TTL and verify the value is re-fetched.
            await new Promise((resolve) => setTimeout(resolve, 1500));
            provider.setPrice('XLM', 0.25);

            const second = await aggregator.getPrice('XLM');
            expect(second).not.toBeNull();
            expect(Number(second?.price)).toBeCloseTo(0.25, 8);
        }, 10000);
    });

    describe('Validator Boundaries', () => {
        it('should reject a price that deviates beyond the configured threshold', async () => {
            const strictValidator = createValidator({
                maxDeviationPercent: 10,
                maxStalenessSeconds: 300,
            });
            const a = new EdgeCaseMockProvider('a', 1);
            const b = new EdgeCaseMockProvider('b', 2);
            a.setPrice('XLM', 0.15);
            b.setPrice('XLM', 1.0); // >10% deviation

            const aggregator = createAggregator([a, b], strictValidator, cache);

            const result = await aggregator.getPrice('XLM');

            // With a strict threshold the outlier must not be silently accepted.
            if (result !== null) {
                expect(Number(result.price)).toBeCloseTo(0.15, 8);
            }
        });

        it('should accept a price within the configured threshold', async () => {
            const strictValidator = createValidator({
                maxDeviationPercent: 10,
                maxStalenessSeconds: 300,
            });
            const a = new EdgeCaseMockProvider('a', 1);
            const b = new EdgeCaseMockProvider('b', 2);
            a.setPrice('XLM', 0.15);
            b.setPrice('XLM', 0.155); // 3% deviation

            const aggregator = createAggregator([a, b], strictValidator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
        });

        it('should reject an exactly stale timestamp at the boundary', async () => {
            const boundaryValidator = createValidator({
                maxDeviationPercent: 100,
                maxStalenessSeconds: 300,
            });
            provider.setPrice('XLM', 0.15);
            provider.setTimestampOffset(-600); // 600s > 300s max staleness

            const aggregator = createAggregator([provider], boundaryValidator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });
    });

    describe('Regression Coverage', () => {
        it('should not leak a cache entry between independent aggregators', async () => {
            const cache1 = createPriceCache(30);
            const cache2 = createPriceCache(30);
            const p1 = new EdgeCaseMockProvider('p1');
            const p2 = new EdgeCaseMockProvider('p2');
            p1.setPrice('XLM', 0.15);
            p2.setPrice('XLM', 0.99);

            const aggregator1 = createAggregator([p1], validator, cache1);
            const aggregator2 = createAggregator([p2], validator, cache2);

            const r1 = await aggregator1.getPrice('XLM');
            const r2 = await aggregator2.getPrice('XLM');

            expect(Number(r1?.price)).toBleCloseTo(0.15, 8);
            expect(Number(r2?.price)).toBeCloseTo(0.99, 8);
        });

        it('should not mutate the input asset array on getPrices', async () => {
            provider.setPrice('XLM', 0.15);
            const aggregator = createAggregator([provider], validator, cache);
            const input = ['XLM', 'XLM'];
            const snapshot = [...input];

            await aggregator.getPrices(input);

            expect(input).toEqual(snapshot);
        });

        it('should return a defined map even when all assets are unsupported', async () => {
            const aggregator = createAggregator([provider], validator, cache);

            const results = await aggregator.getPrices(['NONE', 'NONE2']);

            expect(results).toBeInstanceOf(Map);
            expect(results.size).toBe(0);
        });

        it('should not throw when a getPrice call is made for an unsupported asset', async () => {
            const aggregator = createAggregator([provider], validator, cache);

            await expect(aggregator.getPrice('NOPE')).resolves.toBeNull();
        });

        it('should not throw when getPrices is called with an empty array', async () => {
            const aggregator = createAggregator([provider], validator, cache);

            await expect(aggregator.getPrices([])).resolves.toBeDefined();
        });
    });
});
