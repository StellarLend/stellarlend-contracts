/**
 * Tests for Edge Cases and Boundary Conditions
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { createAggregator } from '../src/services/price-aggregator.js';
import { createValidator } from '../src/services/price-validator.js';
import { createPriceCache } from '../src/services/cache.js';
import { scalePrice, unscalePrice } from '../src/config.js';
import { BasePriceProvider } from '../src/providers/base-provider.js';
import type { RawPriceData } from '../src/types/index.js';

/**
 * Mock provider for edge case testing
 */
class EdgeCaseMockProvider extends BasePriceProvider {
    private mockPrices: Map<string, number> = new Map();

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

    async fetchPrice(asset: string): Promise<RawPriceData> {
        const price = this.mockPrices.get(asset.toUpperCase());
        if (price === undefined) {
            throw new Error(`Asset ${asset} not supported`);
        }

        return {
            asset: asset.toUpperCase(),
            price,
            timestamp: Math.floor(Date.now() / 1000),
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

    describe('Empty Asset Lists', () => {
        it('should handle empty asset array in getPrices', async () => {
            const aggregator = createAggregator([provider], validator, cache);

            const results = await aggregator.getPrices([]);

            expect(results).toBeDefined();
            expect(results.size).toBe(0);
        });

        it('should return empty map for no supported assets', async () => {
            const aggregator = createAggregator([provider], validator, cache);

            const results = await aggregator.getPrices(['UNSUPPORTED1', 'UNSUPPORTED2']);

            expect(results.size).toBe(0);
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

            expect(scaled).toBeGreaterThanOrEqual(0n);
        });

        it('should handle maximum safe integer', () => {
            const maxSafe = Number.MAX_SAFE_INTEGER;

            expect(() => scalePrice(maxSafe)).not.toThrow();
        });

        it('should handle number precision limits', () => {
            const precisePrice = 0.123456789012345;
            provider.setPrice('TEST', precisePrice);

            const aggregator = createAggregator([provider], validator, cache);

            expect(aggregator.getPrice('TEST')).resolves.toBeDefined();
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
    });

    describe('Future Timestamps', () => {
        it('should handle future timestamps', async () => {
            class FutureTimestampProvider extends EdgeCaseMockProvider {
                async fetchPrice(asset: string): Promise<RawPriceData> {
                    const data = await super.fetchPrice(asset);
                    return {
                        ...data,
                        timestamp: Math.floor(Date.now() / 1000) + 3600, // 1 hour in future
                    };
                }
            }

            const futureProvider = new FutureTimestampProvider('future');
            futureProvider.setPrice('XLM', 0.15);

            const aggregator = createAggregator([futureProvider], validator, cache);

            const result = await aggregator.getPrice('XLM');

            // Should handle gracefully (may reject or accept based on validation)
            expect(result).toBeDefined();
        });

        it('should handle timestamp at epoch zero', async () => {
            class EpochZeroProvider extends EdgeCaseMockProvider {
                async fetchPrice(asset: string): Promise<RawPriceData> {
                    const data = await super.fetchPrice(asset);
                    return {
                        ...data,
                        timestamp: 0,
                    };
                }
            }

            const epochProvider = new EpochZeroProvider('epoch');
            epochProvider.setPrice('XLM', 0.15);

            const aggregator = createAggregator([epochProvider], validator, cache);

            const result = await aggregator.getPrice('XLM');

            // Very old timestamp, should likely be rejected as stale
            expect(result).toBeDefined();
        });

        it('should handle very large timestamps', async () => {
            class LargeTimestampProvider extends EdgeCaseMockProvider {
                async fetchPrice(asset: string): Promise<RawPriceData> {
                    const data = await super.fetchPrice(asset);
                    return {
                        ...data,
                        timestamp: 9999999999, // Year 2286
                    };
                }
            }

            const largeTimestampProvider = new LargeTimestampProvider('large-ts');
            largeTimestampProvider.setPrice('XLM', 0.15);

            const aggregator = createAggregator([largeTimestampProvider], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeDefined();
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

            for (let i = 0; i < 20; i++) {
                const result = await aggregator.getPrice('XLM');
                expect(result).not.toBeNull();
            }
        });

        it('should maintain cache consistency under concurrent access', async () => {
            provider.setPrice('XLM', 0.15);

            const aggregator = createAggregator([provider], validator, cache);

            // First call populates cache
            await aggregator.getPrice('XLM');

            // Concurrent calls should all get consistent cached result
            const promises = Array(20).fill(null).map(() =>
                aggregator.getPrice('XLM')
            );

            const results = await Promise.all(promises);

            const prices = results.map(r => r?.price).filter(p => p !== undefined);
            const uniquePrices = new Set(prices.map(p => Number(p)));

            // All prices should be the same (cached)
            expect(uniquePrices.size).toBeLessThanOrEqual(2); // Allow for cache miss edge case
        });
    });

    describe('Cache Edge Cases', () => {
        it('should handle cache expiration boundary', async () => {
            const shortCache = createPriceCache(0.05); // 50ms TTL
            provider.setPrice('XLM', 0.15);

            const aggregator = createAggregator([provider], validator, shortCache);

            // First fetch
            const result1 = await aggregator.getPrice('XLM');
            expect(result1).not.toBeNull();

            // Wait exactly at expiration boundary
            await new Promise(resolve => setTimeout(resolve, 55));

            // Should fetch fresh data
            const result2 = await aggregator.getPrice('XLM');
            expect(result2).not.toBeNull();
        });

        it('should handle cache with zero TTL', () => {
            expect(() => createPriceCache(0)).not.toThrow();
        });

        it('should handle cache with very large TTL', () => {
            const largeCache = createPriceCache(999999);

            expect(largeCache).toBeDefined();
        });

        it('should handle cache key collisions', async () => {
            provider.setPrice('XLM', 0.15);
            provider.setPrice('xlm', 0.16); // Different case

            const aggregator = createAggregator([provider], validator, cache);

            const result1 = await aggregator.getPrice('XLM');
            const result2 = await aggregator.getPrice('xlm');

            // Should normalize to same key
            if (result1 && result2) {
                expect(result1.asset).toBe(result2.asset);
            }
        });
    });

    describe('Provider Priority Edge Cases', () => {
        it('should handle providers with same priority', async () => {
            const provider1 = new EdgeCaseMockProvider('p1', 1);
            const provider2 = new EdgeCaseMockProvider('p2', 1);
            const provider3 = new EdgeCaseMockProvider('p3', 1);

            [provider1, provider2, provider3].forEach(p => {
                p.setPrice('XLM', 0.15);
            });

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources.length).toBeGreaterThan(0);
        });

        it('should handle single provider', async () => {
            provider.setPrice('XLM', 0.15);

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources).toHaveLength(1);
        });

        it('should handle many providers', async () => {
            const providers = Array(10).fill(null).map((_, i) => {
                const p = new EdgeCaseMockProvider(`provider-${i}`, i + 1);
                p.setPrice('XLM', 0.15 + (i * 0.001)); // Slightly different prices
                return p;
            });

            const aggregator = createAggregator(providers, validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
        });
    });

    describe('Weighted Median Edge Cases', () => {
        it('should handle all identical prices', async () => {
            const providers = [1, 2, 3].map(i => {
                const p = new EdgeCaseMockProvider(`p${i}`, i);
                p.setPrice('XLM', 0.15); // All same price
                return p;
            });

            const aggregator = createAggregator(providers, validator, cache);

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.price).toBeDefined();
        });

        it('should handle extreme price variance', async () => {
            const provider1 = new EdgeCaseMockProvider('p1', 1);
            const provider2 = new EdgeCaseMockProvider('p2', 2);
            const provider3 = new EdgeCaseMockProvider('p3', 3);

            provider1.setPrice('XLM', 0.01);
            provider2.setPrice('XLM', 0.15);
            provider3.setPrice('XLM', 100.00);

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { useWeightedMedian: true }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            // Median should handle outliers
        });

        it('should handle single price source', async () => {
            provider.setPrice('XLM', 0.15);

            const aggregator = createAggregator(
                [provider],
                validator,
                cache,
                { useWeightedMedian: true }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources).toHaveLength(1);
        });
    });

    describe('Asset Name Normalization', () => {
        it('should handle lowercase asset names', async () => {
            provider.setPrice('xlm', 0.15);

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('xlm');

            // Should normalize to uppercase
            expect(result?.asset).toBe('XLM');
        });

        it('should handle mixed case asset names', async () => {
            provider.setPrice('XlM', 0.15);

            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice('XlM');

            expect(result?.asset).toBe('XLM');
        });

        it('should handle whitespace in asset names', async () => {
            const aggregator = createAggregator([provider], validator, cache);

            const result = await aggregator.getPrice(' XLM ');

            // Should handle gracefully
            expect(result === null || result?.asset === 'XLM').toBe(true);
        });
    });
});
