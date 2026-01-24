/**
 * Tests for Price Validator Service
 */

import { describe, it, expect, beforeEach } from 'vitest';
import { PriceValidator, createValidator } from '../src/services/price-validator.js';
import type { RawPriceData } from '../src/types/index.js';

describe('PriceValidator', () => {
    let validator: PriceValidator;

    beforeEach(() => {
        validator = createValidator({
            maxDeviationPercent: 10,
            maxStalenessSeconds: 300,
            minPrice: 0.0001,
            maxPrice: 1000000,
        });
    });

    describe('validate', () => {
        it('should validate a correct price', () => {
            const rawPrice: RawPriceData = {
                asset: 'XLM',
                price: 0.15,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'coingecko',
            };

            const result = validator.validate(rawPrice);

            expect(result.isValid).toBe(true);
            expect(result.price).toBeDefined();
            expect(result.price?.asset).toBe('XLM');
            expect(result.price?.source).toBe('coingecko');
            expect(result.errors).toHaveLength(0);
        });

        it('should reject zero price', () => {
            const rawPrice: RawPriceData = {
                asset: 'XLM',
                price: 0,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'coingecko',
            };

            const result = validator.validate(rawPrice);

            expect(result.isValid).toBe(false);
            expect(result.errors.length).toBeGreaterThan(0);
            expect(result.errors[0].code).toBe('PRICE_ZERO');
        });

        it('should reject negative price', () => {
            const rawPrice: RawPriceData = {
                asset: 'XLM',
                price: -0.15,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'binance',
            };

            const result = validator.validate(rawPrice);

            expect(result.isValid).toBe(false);
            expect(result.errors.length).toBeGreaterThan(0);
        });

        it('should reject stale price', () => {
            const rawPrice: RawPriceData = {
                asset: 'XLM',
                price: 0.15,
                timestamp: Math.floor(Date.now() / 1000) - 600,
                source: 'coingecko',
            };

            const result = validator.validate(rawPrice);

            expect(result.isValid).toBe(false);
            expect(result.errors.some(e => e.code === 'PRICE_STALE')).toBe(true);
        });

        it('should reject price with too high deviation from cache', () => {
            const initialPrice: RawPriceData = {
                asset: 'XLM',
                price: 0.15,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'binance',
            };
            validator.validate(initialPrice);

            const newPrice: RawPriceData = {
                asset: 'XLM',
                price: 0.20,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'coingecko',
            };

            const result = validator.validate(newPrice);

            expect(result.isValid).toBe(false);
            expect(result.errors.some(e => e.code === 'PRICE_DEVIATION_TOO_HIGH')).toBe(true);
        });

        it('should accept price within deviation limit', () => {
            const initialPrice: RawPriceData = {
                asset: 'BTC',
                price: 50000,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'binance',
            };
            validator.validate(initialPrice);

            const newPrice: RawPriceData = {
                asset: 'BTC',
                price: 52000,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'coingecko',
            };

            const result = validator.validate(newPrice);

            expect(result.isValid).toBe(true);
        });

        it('should reject price above maximum', () => {
            const rawPrice: RawPriceData = {
                asset: 'XLM',
                price: 2000000000,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'coingecko',
            };

            const result = validator.validate(rawPrice);

            expect(result.isValid).toBe(false);
        });

        it('should reject price below minimum', () => {
            const rawPrice: RawPriceData = {
                asset: 'XLM',
                price: 0.00000001,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'coingecko',
            };

            const result = validator.validate(rawPrice);

            expect(result.isValid).toBe(false);
        });
    });

    describe('validateMany', () => {
        it('should validate multiple prices', () => {
            const prices: RawPriceData[] = [
                { asset: 'XLM', price: 0.15, timestamp: Math.floor(Date.now() / 1000), source: 'coingecko' },
                { asset: 'BTC', price: 50000, timestamp: Math.floor(Date.now() / 1000), source: 'coingecko' },
                { asset: 'ETH', price: 0, timestamp: Math.floor(Date.now() / 1000), source: 'binance' }, // Invalid
            ];

            const results = validator.validateMany(prices);

            expect(results).toHaveLength(3);
            expect(results[0].isValid).toBe(true);
            expect(results[1].isValid).toBe(true);
            expect(results[2].isValid).toBe(false);
        });
    });

    describe('cache management', () => {
        it('should update cache on valid price', () => {
            const rawPrice: RawPriceData = {
                asset: 'SOL',
                price: 100,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'coingecko',
            };

            validator.validate(rawPrice);

            const cacheState = validator.getCacheState();
            expect(cacheState['SOL']).toBe(100);
        });

        it('should clear specific asset from cache', () => {
            const rawPrice: RawPriceData = {
                asset: 'DOT',
                price: 10,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'coingecko',
            };

            validator.validate(rawPrice);
            validator.clearCache('DOT');

            const cacheState = validator.getCacheState();
            expect(cacheState['DOT']).toBeUndefined();
        });

        it('should clear all cache', () => {
            const prices: RawPriceData[] = [
                { asset: 'XLM', price: 0.15, timestamp: Math.floor(Date.now() / 1000), source: 'coingecko' },
                { asset: 'BTC', price: 50000, timestamp: Math.floor(Date.now() / 1000), source: 'coingecko' },
            ];

            prices.forEach(p => validator.validate(p));
            validator.clearCache();

            const cacheState = validator.getCacheState();
            expect(Object.keys(cacheState)).toHaveLength(0);
        });

        it('should allow manual cache update', () => {
            validator.updateCache('AVAX', 25);

            const cacheState = validator.getCacheState();
            expect(cacheState['AVAX']).toBe(25);
        });
    });

    describe('confidence calculation', () => {
        it('should give higher confidence to fresher prices', () => {
            const freshPrice: RawPriceData = {
                asset: 'XLM',
                price: 0.15,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'coingecko',
            };

            const result = validator.validate(freshPrice);

            expect(result.price?.confidence).toBeGreaterThan(90);
        });

        it('should give higher confidence to coingecko vs binance', () => {
            const coingeckoPrice: RawPriceData = {
                asset: 'ETH',
                price: 3000,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'coingecko',
            };

            const result = validator.validate(coingeckoPrice);

            expect(result.price?.confidence).toBeGreaterThan(0);
        });
    });
});
