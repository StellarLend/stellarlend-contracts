/**
 * Tests for Binance Provider
 */

import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import { BinanceProvider, createBinanceProvider } from '../src/providers/binance.js';

// Mock axios
vi.mock('axios', () => ({
    default: {
        get: vi.fn(),
    },
}));

import axios from 'axios';
const mockedAxios = vi.mocked(axios);

describe('BinanceProvider', () => {
    let provider: BinanceProvider;

    beforeEach(() => {
        provider = createBinanceProvider();
        vi.clearAllMocks();
    });

    afterEach(() => {
        vi.restoreAllMocks();
    });

    describe('fetchPrice', () => {
        it('should fetch price for supported asset', async () => {
            const mockResponse = {
                data: {
                    symbol: 'XLMUSDT',
                    lastPrice: '0.15000000',
                    closeTime: 1705900000000, // ms
                },
            };

            mockedAxios.get.mockResolvedValueOnce(mockResponse);

            const result = await provider.fetchPrice('XLM');

            expect(result.asset).toBe('XLM');
            expect(result.price).toBe(0.15);
            expect(result.source).toBe('binance');
            expect(result.timestamp).toBe(1705900000);
        });

        it('should throw error for unsupported asset', async () => {
            await expect(provider.fetchPrice('UNKNOWN')).rejects.toThrow(
                'Asset UNKNOWN not mapped for Binance'
            );
        });

        it('should handle API errors', async () => {
            mockedAxios.get.mockRejectedValueOnce(new Error('Request failed with status code 418'));

            await expect(provider.fetchPrice('BTC')).rejects.toThrow();
        });
    });

    describe('fetchPrices (batch)', () => {
        it('should fetch multiple prices in batch call', async () => {
            const mockResponse = {
                data: [
                    { symbol: 'XLMUSDT', price: '0.15000000' },
                    { symbol: 'BTCUSDT', price: '50000.00000000' },
                    { symbol: 'ETHUSDT', price: '3000.00000000' },
                ],
            };

            mockedAxios.get.mockResolvedValueOnce(mockResponse);

            const results = await provider.fetchPrices(['XLM', 'BTC', 'ETH']);

            expect(results).toHaveLength(3);
            expect(results.find(r => r.asset === 'XLM')?.price).toBe(0.15);
            expect(results.find(r => r.asset === 'BTC')?.price).toBe(50000);
            expect(results.find(r => r.asset === 'ETH')?.price).toBe(3000);
        });

        it('should skip unsupported assets', async () => {
            const mockResponse = {
                data: [
                    { symbol: 'XLMUSDT', price: '0.15000000' },
                ],
            };

            mockedAxios.get.mockResolvedValueOnce(mockResponse);

            const results = await provider.fetchPrices(['XLM', 'INVALID']);

            expect(results).toHaveLength(1);
            expect(results[0].asset).toBe('XLM');
        });
    });

    describe('getSupportedAssets', () => {
        it('should return list of supported assets', () => {
            const assets = provider.getSupportedAssets();

            expect(assets).toContain('XLM');
            expect(assets).toContain('BTC');
            expect(assets).toContain('ETH');
            expect(assets).toContain('SOL');
            expect(assets).toContain('DOGE');
        });
    });

    describe('provider properties', () => {
        it('should have correct name', () => {
            expect(provider.name).toBe('binance');
        });

        it('should have priority 2 (second)', () => {
            expect(provider.priority).toBe(2);
        });

        it('should be enabled', () => {
            expect(provider.isEnabled).toBe(true);
        });

        it('should have generous rate limits', () => {
            // Binance allows 1200 requests per minute
            expect(provider.weight).toBe(0.4);
        });
    });
});
