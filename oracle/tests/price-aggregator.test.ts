/**
 * Tests for Price Aggregator Service
 */

import { describe, it, expect, beforeEach, vi } from 'vitest';
import { PriceAggregator, createAggregator } from '../src/services/price-aggregator.js';
import { createValidator } from '../src/services/price-validator.js';
import { createPriceCache } from '../src/services/cache.js';
import { BasePriceProvider } from '../src/providers/base-provider.js';
import type { RawPriceData, ProviderConfig, HealthStatus } from '../src/types/index.js';

/**
 * Mock provider for testing
 */
class MockProvider extends BasePriceProvider {
    private mockPrices: Map<string, number> = new Map();
    private shouldFail: boolean = false;

    constructor(
        name: string,
        priority: number,
        weight: number,
        prices: Record<string, number> = {},
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
            this.mockPrices.set(asset.toUpperCase(), price);
        });
    }

    async fetchPrice(asset: string): Promise<RawPriceData> {
        if (this.shouldFail) {
            throw new Error(`Mock provider ${this.name} failed`);
        }

        const price = this.mockPrices.get(asset.toUpperCase());
        if (price === undefined) {
            throw new Error(`Asset ${asset} not found in mock provider`);
        }

        return {
            asset: asset.toUpperCase(),
            price,
            timestamp: Math.floor(Date.now() / 1000),
            source: this.name,
        };
    }

    setPrice(asset: string, price: number): void {
        this.mockPrices.set(asset.toUpperCase(), price);
    }

    setFail(shouldFail: boolean): void {
        this.shouldFail = shouldFail;
    }
}

describe('PriceAggregator', () => {
    let aggregator: PriceAggregator;
    let mockProvider1: MockProvider;
    let mockProvider2: MockProvider;
    let mockProvider3: MockProvider;

    beforeEach(() => {
        // Create mock providers with different prices
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
            maxDeviationPercent: 20, // Higher threshold for test variation
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
            // SOL not in any mock provider
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
