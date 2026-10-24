/**
 * Tests for Binance Provider
 */

import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import { BinanceProvider, createBinanceProvider } from '../src/providers/binance.js';

// Mock axios
vi.mock('axios', () => ({
    default: {
        get: vi.fn(),
        isAxiosError: vi.fn(),
    },
}));

import axios from 'axios';
const mockedAxios = vi.mocked(axios);

describe('BinanceProvider', () => {
    let provider: BinanceProvider;

    beforeEach(() => {
        provider = createBinanceProvider();
        vi.clearAllMocks();
        mockedAxios.isAxiosError.mockReturnValue(false);
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
                    quoteVolume: '5000000.00',
                },
            };

            mockedAxios.get.mockResolvedValueOnce(mockResponse);

            const result = await provider.fetchPrice('XLM');

            expect(result.asset).toBe('XLM');
            expect(result.price).toBe(0.15);
            expect(result.source).toBe('binance');
            expect(result.timestamp).toBe(1705900000);
            expect(result.volume24h).toBe(5_000_000n);
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

        it('should reject empty asset string', async () => {
            await expect(provider.fetchPrice('')).rejects.toThrow(
                'Asset  not mapped for Binance'
            );
            expect(mockedAxios.get).not.toHaveBeenCalled();
        });

        it('should reject whitespace-only asset string', async () => {
            await expect(provider.fetchPrice('   ')).rejects.toThrow(
                'Asset     not mapped for Binance'
            );
            expect(mockedAxios.get).not.toHaveBeenCalled();
        });

        it('should reject non-string asset input', async () => {
            await expect(provider.fetchPrice(undefined as unknown as string)).rejects.toThrow();
            await expect(provider.fetchPrice(null as unknown as string)).rejects.toThrow();
            expect(mockedAxios.get).not.toHaveBeenCalled();
        });

        it('should reject when API returns malformed payload', async () => {
            mockedAxios.get.mockResolvedValueOnce({ data: {} });

            await expect(provider.fetchPrice('XLM')).rejects.toThrow();
        });

        it('should reject when lastPrice is not a finite number', async () => {
            mockedAxios.get.mockResolvedValueOnce({
                data: { symbol: 'XLMUSDT', lastPrice: 'not-a-number', closeTime: 1705900000000, quoteVolume: '1' },
            });

            await expect(provider.fetchPrice('XLM')).rejects.toThrow();
        });

        it('should reject when closeTime is missing', async () => {
            mockedAxios.get.mockResolvedValueOnce({
                data: { symbol: 'XLMUSDT', lastPrice: '0.15', quoteVolume: '1' },
            });

            await expect(provider.fetchPrice('XLM')).rejects.toThrow();
        });

        it('should reject when quoteVolume is malformed', async () => {
            mockedAxios.get.mockResolvedValueOnce({
                data: { symbol: 'XLMUSDT', lastPrice: '0.15', closeTime: 1705900000000, quoteVolume: 'abc' },
            });

            await expect(provider.fetchPrice('XLM')).rejects.toThrow();
        });

        it('should handle zero price as a boundary value', async () => {
            mockedAxios.get.mockResolvedValueOnce({
                data: { symbol: 'XLMUSDT', lastPrice: '0.00000000', closeTime: 1705900000000, quoteVolume: '0.00' },
            });

            const result = await provider.fetchPrice('XLM');
            expect(result.price).toBe(0);
            expect(result.volume24h).toBe(0n);
        });

        it('should handle negative price as a boundary value', async () => {
            mockedAxios.get.mockResolvedValueOnce({
                data: { symbol: 'XLMUSDT', lastPrice: '-1.00000000', closeTime: 1705900000000, quoteVolume: '1.00' },
            });

            const result = await provider.fetchPrice('XLM');
            expect(result.price).toBe(-1);
        });

        it('should handle very large quoteVolume without precision loss', async () => {
            mockedAxios.get.mockResolvedValueOnce({
                data: { symbol: 'XLMUSDT', lastPrice: '0.15', closeTime: 1705900000000, quoteVolume: '9007199254740993.00' },
            });

            const result = await provider.fetchPrice('XLM');
            expect(result.volume24h).toBe(9007199254740993n);
        });

        it('should propagate axios error with response status', async () => {
            const axiosErr = Object.assign(new Error('Request failed with status code 429'), {
                isAxiosError: true,
                response: { status: 429, data: { msg: 'rate limited' } },
            });
            mockedAxios.isAxiosError.mockReturnValue(true);
            mockedAxios.get.mockRejectedValueOnce(axiosErr);

            await expect(provider.fetchPrice('BTC')).rejects.toThrow();
        });

        it('should not leak sensitive data in thrown error messages', async () => {
            const axiosErr = Object.assign(new Error('Request failed with status code 401'), {
                isAxiosError: true,
                response: { status: 401, data: { apiKey: 'SECRET-API-KEY' } },
            });
            mockedAxios.isAxiosError.mockReturnValue(true);
            mockedAxios.get.mockRejectedValueOnce(axiosErr);

            try {
                await provider.fetchPrice('BTC');
                throw new Error('expected rejection');
            } catch (err) {
                expect(String((err as Error).message)).not.toContain('SECRET-API-KEY');
            }
        });

        it('should be deterministic across repeated identical calls', async () => {
            const mockResponse = {
                data: {
                    symbol: 'XLMUSDT',
                    lastPrice: '0.15000000',
                    closeTime: 1705900000000,
                    quoteVolume: '5000000.00',
                },
            };
            mockedAxios.get.mockResolvedValue(mockResponse);

            const a = await provider.fetchPrice('XLM');
            const b = await provider.fetchPrice('XLM');
            expect(a).toEqual(b);
        });

        it('should not mutate state on failed fetch (retry yields same result)', async () => {
            mockedAxios.get.mockRejectedValueOnce(new Error('network down'));
            await expect(provider.fetchPrice('XLM')).rejects.toThrow();

            mockedAxios.get.mockResolvedValueOnce({
                data: { symbol: 'XLMUSDT', lastPrice: '0.15', closeTime: 1705900000000, quoteVolume: '1.00' },
            });
            const result = await provider.fetchPrice('XLM');
            expect(result.price).toBe(0.15);
        });

        it('should handle concurrent fetches for the same asset independently', async () => {
            mockedAxios.get.mockResolvedValue({
                data: { symbol: 'XLMUSDT', lastPrice: '0.15', closeTime: 1705900000000, quoteVolume: '1.00' },
            });

            const [a, b, c] = await Promise.all([
                provider.fetchPrice('XLM'),
                provider.fetchPrice('XLM'),
                provider.fetchPrice('XLM'),
            ]);
            expect(a).toEqual(b);
            expect(b).toEqual(c);
        });

        it('should handle concurrent fetches for different assets', async () => {
            mockedAxios.get.mockImplementation(async (_url: string, config: { params?: { symbol?: string } }) => {
                const symbol = config?.params?.symbol ?? 'XLMUSDT';
                const map: Record<string, string> = {
                    XLMUSDT: '0.15',
                    BTCUSDT: '50000',
                    ETHUSDT: '3000',
                };
                return {
                    data: {
                        symbol,
                        lastPrice: map[symbol] ?? '0',
                        closeTime: 1705900000000,
                        quoteVolume: '1.00',
                    },
                };
            });

            const [xlm, btc, eth] = await Promise.all([
                provider.fetchPrice('XLM'),
                provider.fetchPrice('BTC'),
                provider.fetchPrice('ETH'),
            ]);
            expect(xlm.price).toBe(0.15);
            expect(btc.price).toBe(50000);
            expect(eth.price).toBe(3000);
        });
    });

    describe('fetchPrices (batch)', () => {
        it('should fetch multiple prices in batch call', async () => {
            const mockResponse = {
                data: [
                    { symbol: 'XLMUSDT', lastPrice: '0.15000000', closeTime: 1705900000000, quoteVolume: '1000000.00' },
                    { symbol: 'BTCUSDT', lastPrice: '50000.00000000', closeTime: 1705900000000, quoteVolume: '500000000.00' },
                    { symbol: 'ETHUSDT', lastPrice: '3000.00000000', closeTime: 1705900000000, quoteVolume: '200000000.00' },
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
                    { symbol: 'XLMUSDT', lastPrice: '0.15000000', closeTime: 1705900000000, quoteVolume: '1000000.00' },
                ],
            };

            mockedAxios.get.mockResolvedValueOnce(mockResponse);

            const results = await provider.fetchPrices(['XLM', 'INVALID']);

            expect(results).toHaveLength(1);
            expect(results[0].asset).toBe('XLM');
        });

        it('should return empty array for empty input', async () => {
            const results = await provider.fetchPrices([]);
            expect(results).toEqual([]);
            expect(mockedAxios.get).not.toHaveBeenCalled();
        });

        it('should return empty array when all assets are unsupported', async () => {
            const results = await provider.fetchPrices(['NOPE', 'ALSO_NOPE']);
            expect(results).toEqual([]);
            expect(mockedAxios.get).not.toHaveBeenCalled();
        });

        it('should deduplicate duplicate asset inputs', async () => {
            const mockResponse = {
                data: [
                    { symbol: 'XLMUSDT', lastPrice: '0.15000000', closeTime: 1705900000000, quoteVolume: '1000000.00' },
                ],
            };
            mockedAxios.get.mockResolvedValueOnce(mockResponse);

            const results = await provider.fetchPrices(['XLM', 'XLM', 'XLM']);
            expect(results).toHaveLength(1);
            expect(results[0].asset).toBe('XLM');
        });

        it('should skip entries missing from API response', async () => {
            const mockResponse = {
                data: [
                    { symbol: 'XLMUSDT', lastPrice: '0.15000000', closeTime: 1705900000000, quoteVolume: '1000000.00' },
                ],
            };
            mockedAxios.get.mockResolvedValueOnce(mockResponse);

            const results = await provider.fetchPrices(['XLM', 'BTC']);
            expect(results).toHaveLength(1);
            expect(results[0].asset).toBe('XLM');
        });

        it('should reject when API returns non-array payload', async () => {
            mockedAxios.get.mockResolvedValueOnce({ data: { not: 'an array' } });

            await expect(provider.fetchPrices(['XLM', 'BTC'])).rejects.toThrow();
        });

        it('should reject when API request fails', async () => {
            mockedAxios.get.mockRejectedValueOnce(new Error('network down'));

            await expect(provider.fetchPrices(['XLM', 'BTC'])).rejects.toThrow();
        });

        it('should be deterministic across repeated batch calls', async () => {
            const mockResponse = {
                data: [
                    { symbol: 'XLMUSDT', lastPrice: '0.15000000', closeTime: 1705900000000, quoteVolume: '1000000.00' },
                    { symbol: 'BTCUSDT', lastPrice: '50000.00000000', closeTime: 1705900000000, quoteVolume: '500000000.00' },
                ],
            };
            mockedAxios.get.mockResolvedValue(mockResponse);

            const a = await provider.fetchPrices(['XLM', 'BTC']);
            const b = await provider.fetchPrices(['XLM', 'BTC']);
            expect(a).toEqual(b);
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

        it('should return a stable, non-empty list', () => {
            const a = provider.getSupportedAssets();
            const b = provider.getSupportedAssets();
            expect(a).toEqual(b);
            expect(a.length).toBeGreaterThan(0);
            expect(new Set(a).size).toBe(a.length);
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

        it('should expose a positive weight for rate limiting', () => {
            expect(provider.weight).toBeGreaterThan(0);
        });

        it('should expose a positive integer priority', () => {
            expect(Number.isInteger(provider.priority)).toBe(true);
            expect(provider.priority).toBeGreaterThan(0);
        });
    });

    describe('createBinanceProvider', () => {
        it('should return a fresh provider instance each call', () => {
            const a = createBinanceProvider();
            const b = createBinanceProvider();
            expect(a).not.toBe(b);
            expect(a.name).toBe(b.name);
        });
    });
});
