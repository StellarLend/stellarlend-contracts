/**
 * Tests for CoinGecko Provider
 */

import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import { CoinGeckoProvider, createCoinGeckoProvider } from '../src/providers/coingecko.js';

// Mock axios
vi.mock('axios', () => ({
    default: {
        get: vi.fn(),
    },
}));

import axios from 'axios';
const mockedAxios = vi.mocked(axios);

describe('CoinGeckoProvider', () => {
    let provider: CoinGeckoProvider;

    beforeEach(() => {
        provider = createCoinGeckoProvider();
        vi.clearAllMocks();
    });

    afterEach(() => {
        vi.restoreAllMocks();
    });

    describe('fetchPrice', () => {
        it('should fetch price for supported asset', async () => {
            const mockResponse = {
                data: {
                    stellar: {
                        usd: 0.15,
                        last_updated_at: 1705900000,
                    },
                },
            };

            mockedAxios.get.mockResolvedValueOnce(mockResponse);

            const result = await provider.fetchPrice('XLM');

            expect(result.asset).toBe('XLM');
            expect(result.price).toBe(0.15);
            expect(result.source).toBe('coingecko');
            expect(result.timestamp).toBe(1705900000);
        });

        it('should throw error for unsupported asset', async () => {
            await expect(provider.fetchPrice('UNKNOWN')).rejects.toThrow(
                'Asset UNKNOWN not mapped for CoinGecko'
            );
        });

        it('should handle API errors', async () => {
            mockedAxios.get.mockRejectedValueOnce(new Error('Request failed with status code 429'));

            await expect(provider.fetchPrice('BTC')).rejects.toThrow();
        });

        it('should handle missing price data', async () => {
            mockedAxios.get.mockResolvedValueOnce({ data: {} });

            await expect(provider.fetchPrice('ETH')).rejects.toThrow(
                'No price data returned'
            );
        });
    });

    describe('fetchPrices (batch)', () => {
        it('should fetch multiple prices in one call', async () => {
            const mockResponse = {
                data: {
                    stellar: { usd: 0.15, last_updated_at: 1705900000 },
                    bitcoin: { usd: 50000, last_updated_at: 1705900000 },
                    ethereum: { usd: 3000, last_updated_at: 1705900000 },
                },
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
                data: {
                    stellar: { usd: 0.15, last_updated_at: 1705900000 },
                },
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
            expect(assets).toContain('USDC');
        });
    });

    describe('with API key (Pro tier)', () => {
        it('should use pro API URL and include API key header', async () => {
            const proProvider = createCoinGeckoProvider('test-api-key');

            mockedAxios.get.mockResolvedValueOnce({
                data: {
                    stellar: { usd: 0.15, last_updated_at: 1705900000 },
                },
            });

            await proProvider.fetchPrice('XLM');

            expect(mockedAxios.get).toHaveBeenCalledWith(
                expect.stringContaining('pro-api.coingecko.com'),
                expect.objectContaining({
                    headers: expect.objectContaining({
                        'x-cg-pro-api-key': 'test-api-key',
                    }),
                })
            );
        });
    });

    describe('provider properties', () => {
        it('should have correct name', () => {
            expect(provider.name).toBe('coingecko');
        });

        it('should have priority 2', () => {
            expect(provider.priority).toBe(1);
        });

        it('should be enabled', () => {
            expect(provider.isEnabled).toBe(true);
        });
    });

    describe('Rate-limit (429) & Cooldown Handling', () => {
        const createMockAxiosError = (status: number, headers: Record<string, string> = {}) => {
            const error = new Error(`Request failed with status code ${status}`);
            (error as any).response = {
                status,
                headers,
            };
            return error;
        };

        beforeEach(() => {
            vi.useFakeTimers();
        });

        afterEach(() => {
            vi.useRealTimers();
        });

        it('should handle 429 with numeric Retry-After header', async () => {
            const error = createMockAxiosError(429, { 'retry-after': '30' });
            mockedAxios.get.mockRejectedValueOnce(error);

            // Fetch should throw error
            await expect(provider.fetchPrice('XLM')).rejects.toThrow();

            // Cooldown must be set to 30 seconds
            expect(provider.isCooledDown).toBe(true);
            expect(provider.cooldownUntil).toBe(Date.now() + 30000);

            // Subsequent fetch should reject immediately without axios get call
            await expect(provider.fetchPrice('XLM')).rejects.toThrow(/cooldown/);
            expect(mockedAxios.get).toHaveBeenCalledTimes(1); // Still 1

            // Advance timers by 31 seconds
            vi.advanceTimersByTime(31000);
            expect(provider.isCooledDown).toBe(false);

            // Now it should try fetching again
            mockedAxios.get.mockResolvedValueOnce({
                data: { stellar: { usd: 0.16 } },
            });
            const result = await provider.fetchPrice('XLM');
            expect(result.price).toBe(0.16);
            expect(mockedAxios.get).toHaveBeenCalledTimes(2);
        });

        it('should handle 429 with Date-based Retry-After header', async () => {
            const retryDate = new Date(Date.now() + 45000);
            retryDate.setMilliseconds(0);
            const error = createMockAxiosError(429, { 'retry-after': retryDate.toUTCString() });
            mockedAxios.get.mockRejectedValueOnce(error);

            await expect(provider.fetchPrice('XLM')).rejects.toThrow();

            expect(provider.isCooledDown).toBe(true);
            // Allowing 10ms tolerance for timing difference in test
            expect(Math.abs(provider.cooldownUntil - retryDate.getTime())).toBeLessThan(10);

            // Subsequent call skips API request
            await expect(provider.fetchPrice('XLM')).rejects.toThrow(/cooldown/);
            expect(mockedAxios.get).toHaveBeenCalledTimes(1);
        });

        it('should handle 429 without Retry-After header using a default 60s cooldown', async () => {
            const error = createMockAxiosError(429); // no headers
            mockedAxios.get.mockRejectedValueOnce(error);

            await expect(provider.fetchPrice('XLM')).rejects.toThrow();

            expect(provider.isCooledDown).toBe(true);
            expect(provider.cooldownUntil).toBe(Date.now() + 60000);
        });

        it('should support cooldowns during batch fetchPrices', async () => {
            const error = createMockAxiosError(429, { 'retry-after': '120' });
            mockedAxios.get.mockRejectedValueOnce(error);

            await expect(provider.fetchPrices(['XLM', 'BTC'])).rejects.toThrow();

            expect(provider.isCooledDown).toBe(true);
            expect(provider.cooldownUntil).toBe(Date.now() + 120000);

            // Subsequent batch fetch should reject immediately
            await expect(provider.fetchPrices(['XLM', 'BTC'])).rejects.toThrow(/cooldown/);
            expect(mockedAxios.get).toHaveBeenCalledTimes(1);
        });
    });
});
