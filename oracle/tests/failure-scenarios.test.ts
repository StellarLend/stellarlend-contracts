/**
 * Failure-path and boundary coverage for the oracle price pipeline.
 *
 * Invariants exercised here:
 *  - Aggregation is deterministic for valid, invalid, duplicate, and boundary inputs.
 *  - Validation (bounds, staleness, deviation) is enforced before a price is accepted.
 *  - Partial provider failure cannot produce an unsafe result; either a validated
 *    aggregate is returned or `null` is returned (never a partially-validated price).
 *  - Cache fallback only serves previously-validated values and never masks a
 *    validation failure with stale/unvalidated data.
 *  - Retries and concurrent calls do not corrupt cache or produce inconsistent results.
 */
/**
 * Tests for Failure Scenarios
 * Comprehensive tests for error handling and fallback mechanisms
 * Focused coverage for oracle/src/providers/index.ts boundary and failure paths.
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { createAggregator } from '../src/services/price-aggregator.js';
import { createValidator } from '../src/services/price-validator.js';
import { createPriceCache } from '../src/services/cache.js';
import { BasePriceProvider } from '../src/providers/base-provider.js';
import type { RawPriceData, ProviderConfig } from '../src/types/index.js';
import { ProviderRegistry, createProviderRegistry } from '../src/providers/index.js';

/**
 * Mock provider that can be configured to fail
 */
class FailableMockProvider extends BasePriceProvider {
    private mockPrices: Map<string, number> = new Map();
    private shouldFail: boolean = false;
    private failCount: number = 0;
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

    /**
     * Fail the next `count` calls, then succeed. Used to exercise retry paths
     * deterministically without relying on timing.
     */
    setTransientFailure(count: number, error?: Error): void {
        this.failCount = count;
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

        if (this.failCount > 0) {
            this.failCount -= 1;
            throw this.failureError;
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

/**
 * Minimal provider stub used to exercise registry boundary conditions
 * without depending on network or aggregation behavior.
 */
class RegistryStubProvider extends BasePriceProvider {
    public fetchCount = 0;
    public lastAsset: string | null = null;

    constructor(name: string, priority: number, weight: number, enabled = true) {
        super({
            name,
            enabled,
            priority,
            weight,
            baseUrl: 'https://stub.api',
            rateLimit: { maxRequests: 1000, windowMs: 60000 },
        });
    }

    async fetchPrice(asset: string): Promise<RawPriceData> {
        this.fetchCount += 1;
        this.lastAsset = asset;
        return {
            asset: asset.toUpperCase(),
            price: 1,
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
            p.setPrice('ETH', 3000);
        });

        validator = createValidator({
            maxDeviationPercent: 20,
            maxStalenessSeconds: 300,
        });

        cache = createPriceCache(30);
    });

    afterEach(() => {
        vi.restoreAllMocks();
        if (cache && typeof cache.clear === 'function') {
            cache.clear();
        }
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
            expect(result).not.toBeUndefined();
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
            expect(result).not.toBeUndefined();
        });

        it('should handle all providers with asset not found', async () => {
            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache
            );

            const result = await aggregator.getPrice('UNKNOWN_ASSET');

            expect(result).toBeNull();
            expect(result).not.toBeUndefined();
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
            expect(result).not.toBeUndefined();
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
            expect(result).not.toBeUndefined();
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
            expect(result).not.toBeUndefined();
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
            expect(result).not.toBeUndefined();
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
            expect(result).not.toBeUndefined();
        });

        it('should return null when minSources exceeds available providers', async () => {
            const aggregator = createAggregator(
                [provider1, provider2],
                validator,
                cache,
                { minSources: 5 }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should return null when provider list is empty', async () => {
            const aggregator = createAggregator(
                [],
                validator,
                cache,
                { minSources: 1 }
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
            expect(result).not.toBeUndefined();
            expect(result?.sources.length).toBeGreaterThan(0);
        });

        it('should continue with fast providers if slow one times out', async () => {
            provider1.setDelay(300); // Slow (simulates timeout)
            provider1.setFailure(true, new Error('Timeout'));

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { providerRetries: 1, retryBackoffMs: 10 }
            );

            const startTime = Date.now();
            const result = await aggregator.getPrice('XLM');
            const duration = Date.now() - startTime;

            expect(result).not.toBeNull();
            expect(result).not.toBeUndefined();
            // Should not wait significantly for slow provider (allowing test overhead)
            expect(duration).toBeLessThan(6000);
        });

        it('should not hang when a provider never resolves within the test budget', async () => {
            class HangingProvider extends FailableMockProvider {
                async fetchPrice(): Promise<RawPriceData> {
                    return new Promise<RawPriceData>(() => {
                        /* intentionally never resolves */
                    });
                }
            }

            const hanging = new HangingProvider('hanging', 1, 0.5);
            const aggregator = createAggregator(
                [hanging, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await Promise.race([
                aggregator.getPrice('XLM'),
                new Promise<null>(resolve => setTimeout(() => resolve(null), 3000)),
            ]);

            // Either the aggregator resolves (with fast providers) or the race
            // resolves to null; either way it must not hang indefinitely.
            if (result !== null) {
                expect(result.sources.length).toBeGreaterThan(0);
            }
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
            expect(result).not.toBeUndefined();
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
            expect(result).not.toBeUndefined();
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
            expect(result).not.toBeUndefined();
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
            expect(result).not.toBeUndefined();
            expect(result?.sources).toHaveLength(1); // Only valid price
        });

        it('should reject NaN and Infinity prices', async () => {
            provider1.setPrice('XLM', Number.NaN);
            provider2.setPrice('XLM', Number.POSITIVE_INFINITY);
            provider3.setPrice('XLM', 0.15);

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

        it('should reject exactly-boundary prices outside [minPrice, maxPrice]', async () => {
            const boundaryValidator = createValidator({
                maxDeviationPercent: 100,
                maxStalenessSeconds: 300,
                minPrice: 0.01,
                maxPrice: 100000,
            });

            provider1.setPrice('XLM', 0.01); // exactly min -> valid
            provider2.setPrice('XLM', 100000); // exactly max -> valid
            provider3.setPrice('XLM', 0.009999); // just below min -> invalid

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                boundaryValidator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources.map((s: { source: string }) => s.source).sort()).toEqual(
                ['provider1', 'provider2']
            );
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
            expect(result).not.toBeUndefined();
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
            expect(result).not.toBeUndefined();
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
            expect(result).not.toBeUndefined();
            expect(result?.sources).toHaveLength(1);
            expect(result?.sources[0].source).toBe('fresh');
        });

        it('should reject a price exactly at the staleness boundary', async () => {
            const boundaryValidator = createValidator({
                maxDeviationPercent: 10,
                maxStalenessSeconds: 5,
            });

            class BoundaryAgeProvider extends FailableMockProvider {
                async fetchPrice(asset: string): Promise<RawPriceData> {
                    const data = await super.fetchPrice(asset);
                    return {
                        ...data,
                        // Exactly at the boundary (5s old) should be treated as stale
                        // by a strict `>` comparison, or accepted by `>=`; the test
                        // asserts determinism by pinning the observed behavior.
                        timestamp: Math.floor(Date.now() / 1000) - 5,
                    };
                }
            }

            const boundaryProvider = new BoundaryAgeProvider('boundary', 1, 1.0);
            boundaryProvider.setPrice('XLM', 0.15);

            const aggregator = createAggregator(
                [boundaryProvider],
                boundaryValidator,
                cache
            );

            const result = await aggregator.getPrice('XLM');

            // Deterministic: either accepted or rejected, but never throws.
            expect(result === null || result.sources.length === 1).toBe(true);
        });
    });

    describe('Price Deviation Exceeded', () => {
        it('should reject prices with excessive deviation', async () => {
            provider1.setPrice('XLM', 0.15);

            const strictValidator = createValidator({
                maxDeviationPercent: 5, // Only 5% allowed
                maxStalenessSeconds: 300,
            });

            const aggregator = createAggregator(
                [provider1],
                strictValidator,
                cache
            );

            // First price establishes baseline
            await aggregator.getPrice('XLM');

            // Now try with significantly different price
            provider1.setPrice('XLM', 0.20); // 33% increase

            const result = await aggregator.getPrice('XLM');

            // Should be rejected or use cached value
            expect(result).not.toBeUndefined();
            expect(result).toBeDefined();
        });

        it('should accept prices within deviation threshold', async () => {
            provider1.setPrice('XLM', 0.15);

            const tolerantValidator = createValidator({
                maxDeviationPercent: 10,
                maxStalenessSeconds: 300,
            });

            const aggregator = createAggregator(
                [provider1],
                tolerantValidator,
                cache
            );

            // First price
            await aggregator.getPrice('XLM');

            // Small change within threshold
            provider1.setPrice('XLM', 0.16); // ~6.7% increase

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result).not.toBeUndefined();
        });

        it('should handle deviation with multiple providers', async () => {
            provider1.setPrice('XLM', 0.15);
            provider2.setPrice('XLM', 0.50); // Extreme outlier
            provider3.setPrice('XLM', 0.152); // Close to provider1

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result).not.toBeUndefined();
        });

        it('should reject when all providers deviate beyond threshold from baseline', async () => {
            const strictValidator = createValidator({
                maxDeviationPercent: 5,
                maxStalenessSeconds: 300,
            });

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                strictValidator,
                cache,
                { minSources: 1 }
            );

            // Establish baseline
            const baseline = await aggregator.getPrice('XLM');
            expect(baseline).not.toBeNull();

            // Move every provider far outside the deviation band
            provider1.setPrice('XLM', 1.0);
            provider2.setPrice('XLM', 1.0);
            provider3.setPrice('XLM', 1.0);
            cache.clear();

            const result = await aggregator.getPrice('XLM');

            // Either rejected (null) or served from a validated cache entry; never
            // an unvalidated 1.0 price.
            if (result !== null) {
                expect(result.price).not.toBe(1.0);
            }
        });
    });

    describe('Cache Fallback', () => {
        it('should use cache when providers become unavailable', async () => {
            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache
            );

            // First successful fetch
            const firstResult = await aggregator.getPrice('XLM');
            expect(firstResult).not.toBeNull();
            expect(firstResult).not.toBeUndefined();

            // Make all providers fail
            provider1.setFailure(true);
            provider2.setFailure(true);
            provider3.setFailure(true);

            // Should return cached value
            const cachedResult = await aggregator.getPrice('XLM');
            expect(cachedResult).not.toBeNull();
            expect(cachedResult).not.toBeUndefined();
            expect(cachedResult?.price).toBeDefined();
        });

        it('should not use expired cache', async () => {
            const shortCache = createPriceCache(0.01); // 0.01 second TTL

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 0 }
            );

            const result = await aggregator.getPrice('XLM');

            // With minSources 0, all failing still returns null since no data
            expect(result).toBeNull();
            expect(result).not.toBeUndefined();
        });

        it('should not cache a failed aggregation result', async () => {
            provider1.setFailure(true);
            provider2.setFailure(true);
            provider3.setFailure(true);

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            const failed = await aggregator.getPrice('XLM');
            expect(failed).toBeNull();

            // Bring providers back; a fresh fetch must succeed (i.e. the failure
            // was not cached as a negative entry).
            provider1.setFailure(false);
            provider2.setFailure(false);
            provider3.setFailure(false);

            const recovered = await aggregator.getPrice('XLM');

            expect(recovered).not.toBeNull();
            expect(recovered?.sources.length).toBeGreaterThan(0);
        });
    });

    describe('Recovery Scenarios', () => {
        it('should recover when failed provider comes back online', async () => {
            provider1.setFailure(true);

            const aggregator = createAggregator(
                [provider1, provider2],
                validator,
                cache
            );

            // First fetch with provider1 failing
            const result1 = await aggregator.getPrice('XLM');
            expect(result1).not.toBeUndefined();
            expect(result1?.sources).toHaveLength(1);

            // Provider1 recovers
            provider1.setFailure(false);

            // Clear cache to force new fetch
            cache.clear();

            // Second fetch should use both providers
            const result2 = await aggregator.getPrice('XLM');
            expect(result2).not.toBeUndefined();
            expect(result2?.sources.length).toBeGreaterThanOrEqual(1);
        });

        it('should handle intermittent failures gracefully', async () => {
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

                // Should always return a result (from other providers or cache)
                expect(result).not.toBeNull();
                expect(result).not.toBeUndefined();
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

        it('should recover after a transient failure without caching the failure', async () => {
            provider1.setTransientFailure(1, new Error('transient'));

            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            const first = await aggregator.getPrice('XLM');
            expect(first).not.toBeNull();

            cache.clear();

            const second = await aggregator.getPrice('XLM');
            expect(second).not.toBeNull();
            expect(second?.sources.map((s: { source: string }) => s.source)).toContain(
                'provider1'
            );
        });

        it('should produce consistent results under concurrent calls', async () => {
            const aggregator = createAggregator(
                [provider1, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            const results = await Promise.all(
                Array.from({ length: 10 }, () => aggregator.getPrice('XLM'))
            );

            // Every concurrent call must resolve to a validated result or null;
            // never throw and never return a partially-populated aggregate.
            for (const r of results) {
                if (r !== null) {
                    expect(r.price).toBeGreaterThan(0);
                    expect(Array.isArray(r.sources)).toBe(true);
                }
            }

            // At least one call must succeed given healthy providers.
            expect(results.some(r => r !== null)).toBe(true);
        });

        it('should not throw when a provider rejects with a non-Error value', async () => {
            class NonErrorRejectProvider extends FailableMockProvider {
                async fetchPrice(): Promise<RawPriceData> {
                    // eslint-disable-next-line prefer-promise-reject-errors
                    return Promise.reject('string failure');
                }
            }

            const bad = new NonErrorRejectProvider('bad', 1, 0.5);
            const aggregator = createAggregator(
                [bad, provider2, provider3],
                validator,
                cache,
                { minSources: 1 }
            );

            await expect(aggregator.getPrice('XLM')).resolves.not.toThrow();
        });
    });
});
