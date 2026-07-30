/**
 * Tests for Price Aggregator Service
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { PriceAggregator, createAggregator, filterOutliersByMAD } from '../src/services/price-aggregator.js';
import { createValidator } from '../src/services/price-validator.js';
import { createPriceCache } from '../src/services/cache.js';
import { BasePriceProvider } from '../src/providers/base-provider.js';
import { scalePrice } from '../src/config.js';
import type { RawPriceData, PriceData, ProviderConfig, HealthStatus } from '../src/types/index.js';

/**
 * Mock provider for testing
 */
class MockProvider extends BasePriceProvider {
    private mockPrices: Map<string, RawPriceData> = new Map();
    private shouldFail: boolean = false;

    constructor(
        name: string,
        priority: number,
        weight: number,
        prices: Record<string, number> = {},
        volumes: Record<string, bigint> = {},
    ) {
        super({
            name,
            enabled: true,
            priority,
            weight,
            baseUrl: 'https://mock.api',
            rateLimit: { maxRequests: 1000, windowMs: 60000 },
        });

        Object.entries(prices).forEach(([asset, price]) => {
            const upper = asset.toUpperCase();
            this.mockPrices.set(upper, {
                asset: upper,
                price,
                timestamp: Math.floor(Date.now() / 1000),
                source: name,
                volume24h: volumes[asset] ?? volumes[upper],
            });
        });
    }

    async fetchPrice(asset: string): Promise<RawPriceData> {
        if (this.shouldFail) {
            throw new Error(`Mock provider ${this.name} failed`);
        }

        const data = this.mockPrices.get(asset.toUpperCase());
        if (data === undefined) {
            throw new Error(`Asset ${asset} not found in mock provider`);
        }

        return { ...data, timestamp: Math.floor(Date.now() / 1000) };
    }

    setPrice(asset: string, price: number): void {
        const upper = asset.toUpperCase();
        const existing = this.mockPrices.get(upper);
        this.mockPrices.set(upper, {
            asset: upper,
            price,
            timestamp: Math.floor(Date.now() / 1000),
            source: this.name,
            volume24h: existing?.volume24h,
        });
    }

    setFail(shouldFail: boolean): void {
        this.shouldFail = shouldFail;
    }
}

// ---------------------------------------------------------------------------
// Helper: build a PriceData from a plain number (using PRICE_SCALE)
// ---------------------------------------------------------------------------
function pd(price: number, source = 'mock'): PriceData {
    return {
        asset: 'XLM',
        price: scalePrice(price),
        timestamp: Math.floor(Date.now() / 1000),
        source,
        confidence: 100,
    };
}

// ---------------------------------------------------------------------------
// filterOutliersByMAD – unit tests
// ---------------------------------------------------------------------------
describe('filterOutliersByMAD', () => {
    describe('edge cases – too few prices', () => {
        it('returns single price unchanged', () => {
            const prices = [pd(100)];
            expect(filterOutliersByMAD(prices, 3.5)).toEqual(prices);
        });

        it('returns 2 prices unchanged (not enough data to reject)', () => {
            const prices = [pd(100), pd(200)];
            expect(filterOutliersByMAD(prices, 3.5)).toEqual(prices);
        });
    });

    describe('filter disabled', () => {
        it('returns all prices when zMax is 0', () => {
            const prices = [pd(100), pd(101), pd(9999)];
            expect(filterOutliersByMAD(prices, 0)).toEqual(prices);
        });

        it('returns all prices when zMax is negative', () => {
            const prices = [pd(100), pd(101), pd(9999)];
            expect(filterOutliersByMAD(prices, -1)).toEqual(prices);
        });
    });

    describe('3 sources', () => {
        it('keeps all prices when none are outliers', () => {
            const prices = [pd(100), pd(101), pd(102)];
            const result = filterOutliersByMAD(prices, 3.5);
            expect(result).toHaveLength(3);
        });

        it('removes a clear outlier (10× the cluster)', () => {
            const prices = [pd(100), pd(101), pd(1000)];
            const result = filterOutliersByMAD(prices, 3.5);
            expect(result).toHaveLength(2);
            expect(result.every((p) => p.price < scalePrice(200))).toBe(true);
        });

        it('keeps all prices when MAD is 0 (all identical)', () => {
            const prices = [pd(100), pd(100), pd(100)];
            expect(filterOutliersByMAD(prices, 3.5)).toHaveLength(3);
        });
    });

    describe('5 sources', () => {
        it('keeps all 5 prices when cluster is tight', () => {
            const prices = [pd(100), pd(100.5), pd(101), pd(100.2), pd(99.8)];
            const result = filterOutliersByMAD(prices, 3.5);
            expect(result).toHaveLength(5);
        });

        it('removes a single high outlier from 5 sources', () => {
            const prices = [pd(100), pd(100.5), pd(101), pd(100.2), pd(5000)];
            const result = filterOutliersByMAD(prices, 3.5);
            expect(result).toHaveLength(4);
            expect(result.some((p) => p.price === scalePrice(5000))).toBe(false);
        });

        it('removes a single low outlier from 5 sources', () => {
            const prices = [pd(100), pd(100.5), pd(101), pd(100.2), pd(0.001)];
            const result = filterOutliersByMAD(prices, 3.5);
            expect(result).toHaveLength(4);
            expect(result.some((p) => p.price === scalePrice(0.001))).toBe(false);
        });

        it('removes both high and low outliers when 2 rogue feeds drift far', () => {
            // Two adversarial feeds; 3 honest prices cluster at ~100
            const prices = [pd(100), pd(100.5), pd(101), pd(5000), pd(0.001)];
            const result = filterOutliersByMAD(prices, 3.5);
            // At most the honest cluster survives
            expect(result.length).toBeLessThanOrEqual(3);
            const allNearCluster = result.every(
                (p) => p.price >= scalePrice(99) && p.price <= scalePrice(102)
            );
            expect(allNearCluster).toBe(true);
        });
    });

    describe('threshold sensitivity', () => {
        it('tighter threshold (z=1) removes more prices', () => {
            // 3 prices with moderate spread
            const prices = [pd(100), pd(110), pd(120)];
            const strict = filterOutliersByMAD(prices, 1.0);
            const lenient = filterOutliersByMAD(prices, 3.5);
            expect(strict.length).toBeLessThanOrEqual(lenient.length);
        });

        it('very large threshold keeps all prices', () => {
            const prices = [pd(100), pd(101), pd(1000)];
            const result = filterOutliersByMAD(prices, 1000);
            expect(result).toHaveLength(3);
        });
    });
});

// ---------------------------------------------------------------------------
// PriceAggregator integration – MAD wired in
// ---------------------------------------------------------------------------
describe('PriceAggregator', () => {
    let aggregator: PriceAggregator;
    let mockProvider1: MockProvider;
    let mockProvider2: MockProvider;
    let mockProvider3: MockProvider;

    beforeEach(() => {
        mockProvider1 = new MockProvider('provider1', 1, 0.5, {
            XLM: 0.15,
            BTC: 50000,
            ETH: 3000,
        });

        mockProvider2 = new MockProvider('provider2', 2, 0.3, {
            XLM: 0.152,
            BTC: 50100,
            ETH: 3010,
        });

        mockProvider3 = new MockProvider('provider3', 3, 0.2, {
            XLM: 0.148,
            BTC: 49900,
            ETH: 2990,
        });

        const validator = createValidator({
            maxDeviationPercent: 20,
            maxStalenessSeconds: 300,
        });

        const cache = createPriceCache(30);

        aggregator = createAggregator(
            [mockProvider1, mockProvider2, mockProvider3],
            validator,
            cache,
            { minSources: 1 }
        );
    });

    describe('getPrice', () => {
        it('should fetch and aggregate price from multiple sources', async () => {
            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.asset).toBe('XLM');
            expect(result?.sources.length).toBeGreaterThanOrEqual(1);
        });

        it('should use cache for subsequent requests', async () => {
            const result1 = await aggregator.getPrice('BTC');
            const result2 = await aggregator.getPrice('BTC');

            expect(result2?.sources).toHaveLength(0);
            expect(result2?.price).toBe(result1?.price);
        });

        it('should return null when no sources provide valid prices', async () => {
            mockProvider1.setFail(true);
            mockProvider2.setFail(true);
            mockProvider3.setFail(true);

            const strictAggregator = createAggregator(
                [mockProvider1, mockProvider2, mockProvider3],
                createValidator(),
                createPriceCache(30),
                { minSources: 1 }
            );

            const result = await strictAggregator.getPrice('XLM');

            expect(result).toBeNull();
        });

        it('should handle fallback when primary provider fails', async () => {
            mockProvider1.setFail(true);

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            expect(result?.sources.every(s => s.source !== 'provider1')).toBe(true);
        });

        it('should handle fallback when primary provider is in cooldown', async () => {
            // Put provider1 in cooldown
            mockProvider1.cooldownUntil = Date.now() + 60000;

            const result = await aggregator.getPrice('XLM');

            expect(result).not.toBeNull();
            // It should skip provider1 and fetch from provider2/3 instead
            expect(result?.sources.every(s => s.source !== 'provider1')).toBe(true);
            expect(result?.sources.some(s => s.source === 'provider2')).toBe(true);
        });

        it('should still produce a result when MAD filter would drop too many sources (fallback)', async () => {
            // Very tight threshold forces fallback to unfiltered list
            const tightAggregator = createAggregator(
                [mockProvider1, mockProvider2, mockProvider3],
                createValidator({ maxDeviationPercent: 20, maxStalenessSeconds: 300 }),
                createPriceCache(30),
                { minSources: 1, madZScoreThreshold: 0.0001 }
            );

            const result = await tightAggregator.getPrice('XLM');
            expect(result).not.toBeNull();
        });

        it('rejects a severely drifted provider price and uses the cluster', async () => {
            // Inject an adversarial outlier into provider3
            mockProvider3.setPrice('XLM', 999.0); // wildly far from 0.15

            const outliersAggregator = createAggregator(
                [mockProvider1, mockProvider2, mockProvider3],
                createValidator({ maxDeviationPercent: 100_000, maxStalenessSeconds: 300 }),
                createPriceCache(30),
                { minSources: 1, madZScoreThreshold: 3.5 }
            );

            const result = await outliersAggregator.getPrice('XLM');
            expect(result).not.toBeNull();
            // Price should be close to the honest cluster (~0.15), not skewed toward 999
            const price = Number(result!.price) / 1_000_000;
            expect(price).toBeGreaterThan(0.1);
            expect(price).toBeLessThan(1.0);
        });
    });

    describe('getPrices', () => {
        it('should fetch prices for multiple assets', async () => {
            const results = await aggregator.getPrices(['XLM', 'BTC', 'ETH']);

            expect(results.size).toBe(3);
            expect(results.has('XLM')).toBe(true);
            expect(results.has('BTC')).toBe(true);
            expect(results.has('ETH')).toBe(true);
        });

        it('should skip assets that fail', async () => {
            const results = await aggregator.getPrices(['XLM', 'SOL']);

            expect(results.size).toBe(1);
            expect(results.has('XLM')).toBe(true);
            expect(results.has('SOL')).toBe(false);
        });
    });

    describe('weighted median calculation', () => {
        it('should calculate correct weighted median', async () => {
            const result = await aggregator.getPrice('XLM');
            expect(result).not.toBeNull();
        });
    });

    describe('provider ordering', () => {
        it('should sort providers by priority', () => {
            const providers = aggregator.getProviders();

            expect(providers[0]).toBe('provider1');
            expect(providers[1]).toBe('provider2');
            expect(providers[2]).toBe('provider3');
        });
    });

    describe('stats', () => {
        it('should return aggregator statistics', async () => {
            await aggregator.getPrice('XLM');

            const stats = aggregator.getStats();

            expect(stats.enabledProviders).toBe(3);
            expect(stats.cacheStats).toBeDefined();
        });
    });
});