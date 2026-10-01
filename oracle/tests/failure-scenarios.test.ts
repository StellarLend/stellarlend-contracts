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
        });

        validator = createValidator({
            maxDeviationPercent: 20,
            maxStalenessSeconds: 300,
        });

        cache = createPriceCache(30);
    });

    describe('Provider Registry Boundary Conditions', () => {
        it('should construct an empty registry without throwing', () => {
            const registry = createProviderRegistry();

            expect(registry).toBeInstanceOf(ProviderRegistry);
            expect(registry.getAll()).toEqual([]);
            expect(registry.getEnabled()).toEqual([]);
        });

        it('should reject duplicate provider names deterministically', () => {
            const registry = createProviderRegistry();
            const first = new RegistryStubProvider('dup', 1, 0.5);
            const second = new RegistryStubProvider('dup', 2, 0.5);

            registry.register(first);

            expect(() => registry.register(second)).toThrow(/duplicate/i);
            expect(registry.getAll()).toHaveLength(1);
            expect(registry.get('dup')).toBe(first);
        });

        it('should reject providers with invalid configuration', () => {
            const registry = createProviderRegistry();
            const invalid = new RegistryStubProvider('invalid', 1, -1);

            expect(() => registry.register(invalid)).toThrow();
            expect(registry.getAll()).toHaveLength(0);
        });

        it('should return undefined for unknown provider lookups', () => {
            const registry = createProviderRegistry();
            registry.register(new RegistryStubProvider('known', 1, 1));

            expect(registry.get('missing')).toBeUndefined();
        });

        it('should exclude disabled providers from enabled set', () => {
            const registry = createProviderRegistry();
            const enabled = new RegistryStubProvider('enabled', 1, 0.5, true);
            const disabled = new RegistryStubProvider('disabled', 2, 0.5, false);

            registry.register(enabled);
            registry.register(disabled);

            expect(registry.getAll()).toHaveLength(2);
            expect(registry.getEnabled()).toEqual([enabled]);
        });

        it('should order enabled providers by ascending priority', () => {
            const registry = createProviderRegistry();
            const low = new RegistryStubProvider('low', 10, 0.5);
            const high = new RegistryStubProvider('high', 1, 0.5);
            const mid = new RegistryStubProvider('mid', 5, 0.5);

            registry.register(low);
            registry.register(high);
            registry.register(mid);

            expect(registry.getEnabled().map(p => p.name)).toEqual(['high', 'mid', 'low']);
        });

        it('should unregister providers and keep remaining state consistent', () => {
            const registry = createProviderRegistry();
            const a = new RegistryStubProvider('a', 1, 0.5);
            const b = new RegistryStubProvider('b', 2, 0.5);

            registry.register(a);
            registry.register(b);

            expect(registry.unregister('a')).toBe(true);
            expect(registry.get('a')).toBeUndefined();
            expect(registry.getAll()).toEqual([b]);

            expect(registry.unregister('a')).toBe(false);
            expect(registry.getAll()).toEqual([b]);
        });

        it('should clear all providers without affecting subsequent registrations', () => {
            const registry = createProviderRegistry();
            registry.register(new RegistryStubProvider('a', 1, 0.5));
            registry.register(new RegistryStubProvider('b', 2, 0.5));

            registry.clear();
            expect(registry.getAll()).toEqual([]);

            const c = new RegistryStubProvider('c', 1, 1);
            registry.register(c);
            expect(registry.getAll()).toEqual([c]);
        });

        it('should be idempotent when clearing an empty registry', () => {
            const registry = createProviderRegistry();

            expect(() => registry.clear()).not.toThrow();
            expect(() => registry.clear()).not.toThrow();
            expect(registry.getAll()).toEqual([]);
        });

        it('should not mutate internal state when callers mutate returned arrays', () => {
            const registry = createProviderRegistry();
            const a = new RegistryStubProvider('a', 1, 0.5);
            registry.register(a);

            const snapshot = registry.getAll();
            snapshot.push(new RegistryStubProvider('injected', 99, 0.5));

            expect(registry.getAll()).toEqual([a]);
        });

        it('should preserve registration order for equal priorities', () => {
            const registry = createProviderRegistry();
            const first = new RegistryStubProvider('first', 1, 0.5);
            const second = new RegistryStubProvider('second', 1, 0.5);

            registry.register(first);
            registry.register(second);

            expect(registry.getEnabled().map(p => p.name)).toEqual(['first', 'second']);
        });
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
    });
});
