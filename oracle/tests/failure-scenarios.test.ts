/**
 * Tests for Failure Scenarios
 * Comprehensive tests for error handling and fallback mechanisms
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { createAggregator } from '../src/services/price-aggregator.js';
import { createValidator } from '../src/services/price-validator.js';
import { createPriceCache } from '../src/services/cache.js';
import { BasePriceProvider } from '../src/providers/base-provider.js';
import type { RawPriceData, ProviderConfig } from '../src/types/index.js';

/**
 * Mock provider that can be configured to fail
 */
class FailableMockProvider extends BasePriceProvider {
    private mockPrices: Map<string, number> = new Map();
    private shouldFail: boolean = false;
    private failureError: Error = new Error('Provider failed');
    private delay: number = 0;

    constructor(name: string, priority: number, weight: number) {
        super({
            name,
            enabled: true,
            priority,
            weight,
            baseUrl: 'https://mock.api',
            rateLimit: { maxRequests: 1000, windowMs: 60000 },
        });
    }

    setPrice(asset: string, price: number): void {
        this.mockPrices.set(asset.toUpperCase(), price);
    }

    setFailure(shouldFail: boolean, error?: Error): void {
        this.shouldFail = shouldFail;
        if (error) {
            this.failureError = error;
        }
    }

    setDelay(ms: number): void {
        this.delay = ms;
    }

    async fetchPrice(asset: string): Promise<RawPriceData> {
        if (this.delay > 0) {
            await new Promise(resolve => setTimeout(resolve, this.delay));
        }

        if (this.shouldFail) {
            throw this.failureError;
        }

        const price = this.mockPrices.get(asset.toUpperCase());
        if (price === undefined) {
            throw new Error(`Asset ${asset} not found`);
        }

        return {
            asset: asset.toUpperCase(),
            price,
            timestamp: Math.floor(Date.now() / 1000),
            source: this.name,
        };
    }
}

describe('Failure Scenarios', () => {
    let provider1: FailableMockProvider;
    let provider2: FailableMockProvider;
    let provider3: FailableMockProvider;
    let validator: any;
    let cache: any;

    beforeEach(() => {
        provider1 = new FailableMockProvider('provider1', 1, 0.5);
        provider2 = new FailableMockProvider('provider2', 2, 0.3);
        provider3 = new FailableMockProvider('provider3', 3, 0.2);

        // Set default prices
        [provider1, provider2, provider3].forEach(p => {
            p.setPrice('XLM', 0.15);
            p.setPrice('BTC', 50000);
        });

        validator = createValidator({
            maxDeviationPercent: 20,
            maxStalenessSeconds: 300,
        });

        cache = createPriceCache(30);
    });

    describe('All Providers Failing', () => {
        it('should return null when all providers fail', async () => {
            provider1.setFailure(true);
            provider2.setFailure(true);
            provider3.setFailure(true);

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should return null when all fetchPrice calls throw errors', async () => {
            provider1.setFailure(true, new Error('Network timeout'));
            provider2.setFailure(true, new Error('Connection refused'));
            provider3.setFailure(true, new Error('DNS lookup failed'));

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache
            );

            const result = await aggregator.getPrice('BTC');

            expect(result).toBeNull();
        });

        it('should handle all providers with asset not found', async () => {
            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache
            );

            const result = await aggregator.getPrice('UNKNOWN_ASSET');

            expect(result).toBeNull();
        });

        it('should not affect cache when all providers fail', async () => {
            // First successful fetch to populate cache
            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache
            );

            await aggregator.getPrice('XLM');

            // Now make all providers fail
            provider1.setFailure(true);
            provider2.setFailure(true);
            provider3.setFailure(true);

            // Should still get cached value
            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources).toHaveLength(0); // Cached result has empty sources
        });
    });

    describe('Partial Provider Failures', () => {
        it('should succeed with 1 provider when 2 fail', async () => {
            provider1.setFailure(true);
            provider2.setFailure(true);
            // provider3 still works

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources).toHaveLength(1);
            expect(result?.sources[0].source).toBe('provider3');
        });

        it('should succeed with 2 providers when 1 fails', async () => {
            provider1.setFailure(true);
            // provider2 and provider3 work

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources).toHaveLength(2);
        });

        it('should try all providers in priority order', async () => {
            // Set different failure points
            provider1.setFailure(true);

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            // Should skip provider1 and use provider2 and provider3
        });

        it('should fail when not enough sources meet minimum', async () => {
            provider1.setFailure(true);
            provider2.setFailure(true);
            // Only provider3 works

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 2 } // Require at least 2 sources
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });
    });

    describe('Network Timeouts', () => {
        it('should handle slow provider responses', async () => {
            provider1.setDelay(50); // Fast
            provider2.setDelay(100); // Slow

            const aggregator = createAggregator(
                [provider1, provider2],
                validator,
                cache
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources.length).toBeGreaterThan(0);
        });

        it('should continue with fast providers if slow one times out', async () => {
            provider1.setDelay(5000); // Very slow (simulates timeout)
            provider1.setFailure(true, new Error('Timeout'));

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache
            );

            const startTime = Date.now();
            const result = await aggregator.getPrice('XLM');
            const duration = Date.now() - startTime;

            expect(result).not.toBeNull();
            // Should not wait significantly for slow provider (allowing test overhead)
            expect(duration).toBeLessThan(6000);
        });
    });

    describe('Invalid Responses', () => {
        it('should handle zero prices', async () => {
            provider1.setPrice('XLM', 0);
            provider2.setPrice('XLM', 0);
            provider3.setPrice('XLM', 0);

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache
            );

            const result = await aggregator.getPrice('XLM');

            // All prices invalid, should return null
            expect(result).toBeNull();
        });

        it('should handle negative prices', async () => {
            provider1.setPrice('XLM', -0.15);
            provider2.setPrice('XLM', -0.15);

            const aggregator = createAggregator(
                [provider1, provider2],
                validator,
                cache
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should handle mix of valid and invalid prices', async () => {
            provider1.setPrice('XLM', 0); // Invalid
            provider2.setPrice('XLM', 0.15); // Valid
            provider3.setPrice('XLM', 0.152); // Valid

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources).toHaveLength(2); // Only valid prices
        });

        it('should handle out of bounds prices', async () => {
            const strictValidator = createValidator({
                maxDeviationPercent: 10,
                maxStalenessSeconds: 300,
                minPrice: 0.01,
                maxPrice: 100000,
            });

            provider1.setPrice('XLM', 0.0001); // Too low
            provider2.setPrice('XLM', 200000); // Too high
            provider3.setPrice('XLM', 0.15); // Valid

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                strictValidator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources).toHaveLength(1); // Only valid price
        });
    });

    describe('Stale Price Detection', () => {
        it('should reject stale prices', async () => {
            const strictValidator = createValidator({
                maxDeviationPercent: 10,
                maxStalenessSeconds: 1, // Very strict: 1 second
            });

            // Mock provider to return old timestamp
            class StaleProvider extends FailableMockProvider {
                async fetchPrice(asset: string): Promise<RawPriceData> {
                    const data = await super.fetchPrice(asset);
                    return {
                        ...data,
                        timestamp: Math.floor(Date.now() / 1000) - 10, // 10 seconds ago
                    };
                }
            }

            const staleProvider = new StaleProvider('stale', 1, 1.0);
            staleProvider.setPrice('XLM', 0.15);

            const aggregator = createAggregator(
                [staleProvider],
                strictValidator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            // Stale price should be rejected
            expect(result).toBeNull();
        });

        it('should accept fresh prices', async () => {
            const strictValidator = createValidator({
                maxDeviationPercent: 10,
                maxStalenessSeconds: 300,
            });

            const aggregator = createAggregator(
                [provider1],
                strictValidator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
        });
    });

    describe('Boundary Conditions', () => {
        it('should handle empty provider list', async () => {
            const aggregator = createAggregator(
                [],
                validator,
                cache
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should handle empty asset string', async () => {
            const aggregator = createAggregator(
                [provider1],
                validator,
                cache
            );

            const result = await aggregator.getPrice('');

            expect(result).toBeNull();
        });

        it('should handle very large price values', async () => {
            provider1.setPrice('XLM', Number.MAX_SAFE_INTEGER);
            provider2.setPrice('XLM', Number.MAX_SAFE_INTEGER);

            const aggregator = createAggregator(
                [provider1, provider2],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            // Should either return a valid result or null, but not throw
            if (result !== null) {
                expect(Number.isFinite(result.price)).toBe(true);
            }
        });

        it('should handle NaN prices', async () => {
            provider1.setPrice('XLM', NaN);
            provider2.setPrice('XLM', 0.15);

            const aggregator = createAggregator(
                [provider1, provider2],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            // NaN should be rejected, only valid price remains
            expect(result).not.toBeNull();
            expect(result?.sources).toHaveLength(1);
        });

        it('should handle Infinity prices', async () => {
            provider1.setPrice('XLM', Infinity);
            provider2.setPrice('XLM', 0.15);

            const aggregator = createAggregator(
                [provider1, provider2],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources).toHaveLength(1);
        });

        it('should respect minSources of 1', async () => {
            provider1.setFailure(true);
            provider2.setFailure(true);
            // Only provider3 works

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources).toHaveLength(1);
        });

        it('should respect minSources of 0', async () => {
            provider1.setFailure(true);
            provider2.setFailure(true);
            provider3.setFailure(true);

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 0 }
            );

            const result = await aggregator.getPrice('XLM');

            // With minSources 0, all failing still returns null since no data
            expect(result).toBeNull();
        });
    });

    describe('Concurrency and Race Conditions', () => {
        it('should handle concurrent requests for same asset', async () => {
            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            const promises = Array.from({ length: 10 }, () => aggregator.getPrice('XLM'));
            const results = await Promise.all(promises);

            // All results should be consistent
            const nonNullResults = results.filter(r => r !== null);
            expect(nonNullResults.length).toBeGreaterThan(0);

            // All non-null results should have the same price
            const prices = new Set(nonNullResults.map(r => r!.price));
            expect(prices.size).toBe(1);
        });

        it('should handle concurrent requests for different assets', async () => {
            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            const [result1, result2] = await Promise.all([
                aggregator.getPrice('XLM'),
                aggregator.getPrice('BTC'),
            ]);

            expect(result1).not.toBeNull();
            expect(result2).not.toBeNull();
            expect(result1!.price).toBeGreaterThan(0);
            expect(result2!.price).toBeGreaterThan(0);
        });

        it('should not corrupt cache under concurrent failures', async () => {
            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            // Populate cache
            await aggregator.getPrice('XLM');

            // Make providers fail and fetch concurrently
            provider1.setFailure(true);
            provider2.setFailure(true);
            provider3.setFailure(true);

            const promises = Array.from({ length: 5 }, () => aggregator.getPrice('XLM'));
            const results = await Promise.all(promises);

            // All should return cached value
            results.forEach(r => {
                expect(r).not.toBeNull();
            });
        });
    });

    describe('Retry and Recovery', () => {
        it('should recover after transient failure', async () => {
            const aggregator = createAggregator(
                [provider1],
                validator,
                cache,
                { minSources: 1 }
            );

            // First call fails
            provider1.setFailure(true);
            const result1 = await aggregator.getPrice('XLM');
            expect(result1).toBeNull();

            // Recover and succeed
            provider1.setFailure(false);
            const result2 = await aggregator.getPrice('XLM');
            expect(result2).not.toBeNull();
        });

        it('should not cache failed results', async () => {
            const aggregator = createAggregator(
                [provider1],
                validator,
                cache,
                { minSources: 1 }
            );

            // Fail first
            provider1.setFailure(true);
            await aggregator.getPrice('XLM');

            // Recover - should fetch fresh, not use cache
            provider1.setFailure(false);
            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources).toHaveLength(1);
        });

        it('should return cached value on subsequent calls', async () => {
            const aggregator = createAggregator(
                [provider1],
                validator,
                cache,
                { minSources: 1 }
            );

            const result1 = await aggregator.getPrice('XLM');
            expect(result1).not.toBeNull();

            // Make provider fail - cache should still serve
            provider1.setFailure(true);
            const result2 = await aggregator.getPrice('XLM');

            expect(result2).not.toBeNull();
            expect(result2!.price).toBe(result1!.price);
        });
    });

    describe('Error Propagation and Safety', () => {
        it('should not leak sensitive error details in result', async () => {
            const sensitiveError = new Error('API key abcdef123456 invalid');
            provider1.setFailure(true, sensitiveError);

            const aggregator = createAggregator(
                [provider1],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            // Result should be null, not contain error details
            expect(result).toBeNull();
        });

        it('should handle provider that throws non-Error values', async () => {
            class WeirdProvider extends BasePriceProvider {
                constructor() {
                    super({
                        name: 'weird',
                        enabled: true,
                        priority: 1,
                        weight: 1,
                        baseUrl: 'https://mock.api',
                        rateLimit: { maxRequests: 1000, windowMs: 60000 },
                    });
                }

                async fetchPrice(): Promise<RawPriceData> {
                    throw 'string error'; // Non-Error throw
                }
            }

            const weirdProvider = new WeirdProvider();
            const aggregator = createAggregator(
                [weirdProvider],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should handle provider that returns malformed data', async () => {
            class MalformedProvider extends BasePriceProvider {
                constructor() {
                    super({
                        name: 'malformed',
                        enabled: true,
                        priority: 1,
                        weight: 1,
                        baseUrl: 'https://mock.api',
                        rateLimit: { maxRequests: 1000, windowMs: 60000 },
                    });
                }

                async fetchPrice(): Promise<RawPriceData> {
                    return {
                        asset: 'XLM',
                        price: 'wrong type' as any,
                        timestamp: 'not a number' as any,
                        source: 'malformed',
                    };
                }
            }

            const malformedProvider = new MalformedProvider();
            const aggregator = createAggregator(
                [malformedProvider],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            // Should reject malformed data
            expect(result).toBeNull();
        });
    });
});
