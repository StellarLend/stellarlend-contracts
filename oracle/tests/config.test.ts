/**
 * Tests for Configuration Loading and Validation
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
    loadConfig,
    getAssetMapping,
    isSupportedAsset,
    scalePrice,
    unscalePrice,
    PRICE_SCALE,
    ASSET_MAPPINGS,
} from '../src/config.js';

describe('Configuration', () => {
    const originalEnv = process.env;

    beforeEach(() => {
        // Reset environment before each test
        process.env = { ...originalEnv };
    });

    afterEach(() => {
        // Restore original environment
        process.env = originalEnv;
    });

    describe('loadConfig', () => {
        it('should load valid configuration with all required fields', () => {
            process.env.STELLAR_NETWORK = 'testnet';
            process.env.STELLAR_RPC_URL = 'https://soroban-testnet.stellar.org';
            process.env.CONTRACT_ID = 'CTEST123456789';
            process.env.ADMIN_SECRET_KEY = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';

            const config = loadConfig();

            expect(config.stellarNetwork).toBe('testnet');
            expect(config.stellarRpcUrl).toBe('https://soroban-testnet.stellar.org');
            expect(config.contractId).toBe('CTEST123456789');
            expect(config.adminSecretKey).toBe('STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789');
        });

        it('should use default values when optional fields are missing', () => {
            process.env.CONTRACT_ID = 'CTEST123456789';
            process.env.ADMIN_SECRET_KEY = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';

            const config = loadConfig();

            expect(config.stellarNetwork).toBe('testnet');
            expect(config.stellarRpcUrl).toBe('https://soroban-testnet.stellar.org');
            expect(config.cacheTtlSeconds).toBe(30);
            expect(config.updateIntervalMs).toBe(60000);
            expect(config.maxPriceDeviationPercent).toBe(10);
            expect(config.priceStaleThresholdSeconds).toBe(300);
            expect(config.logLevel).toBe('info');
        });

        it('should override defaults with provided values', () => {
            process.env.CONTRACT_ID = 'CTEST123456789';
            process.env.ADMIN_SECRET_KEY = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';
            process.env.CACHE_TTL_SECONDS = '60';
            process.env.UPDATE_INTERVAL_MS = '120000';
            process.env.MAX_PRICE_DEVIATION_PERCENT = '15';
            process.env.PRICE_STALENESS_THRESHOLD_SECONDS = '600';
            process.env.LOG_LEVEL = 'debug';

            const config = loadConfig();

            expect(config.cacheTtlSeconds).toBe(60);
            expect(config.updateIntervalMs).toBe(120000);
            expect(config.maxPriceDeviationPercent).toBe(15);
            expect(config.priceStaleThresholdSeconds).toBe(600);
            expect(config.logLevel).toBe('debug');
        });

        it('should throw error when CONTRACT_ID is missing', () => {
            process.env.ADMIN_SECRET_KEY = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';
            delete process.env.CONTRACT_ID;

            expect(() => loadConfig()).toThrow('Invalid environment configuration');
        });

        it('should throw error when ADMIN_SECRET_KEY is missing', () => {
            process.env.CONTRACT_ID = 'CTEST123456789';
            delete process.env.ADMIN_SECRET_KEY;

            expect(() => loadConfig()).toThrow('Invalid environment configuration');
        });

        it('should accept mainnet as network option', () => {
            process.env.STELLAR_NETWORK = 'mainnet';
            process.env.CONTRACT_ID = 'CTEST123456789';
            process.env.ADMIN_SECRET_KEY = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';

            const config = loadConfig();

            expect(config.stellarNetwork).toBe('mainnet');
        });

        it('should include CoinGecko provider configuration', () => {
            process.env.CONTRACT_ID = 'CTEST123456789';
            process.env.ADMIN_SECRET_KEY = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';

            const config = loadConfig();

            const coingeckoProvider = config.providers.find(p => p.name === 'coingecko');
            expect(coingeckoProvider).toBeDefined();
            expect(coingeckoProvider?.enabled).toBe(true);
            expect(coingeckoProvider?.priority).toBe(1);
            expect(coingeckoProvider?.baseUrl).toBe('https://api.coingecko.com/api/v3');
        });

        it('should use pro CoinGecko API when API key is provided', () => {
            process.env.CONTRACT_ID = 'CTEST123456789';
            process.env.ADMIN_SECRET_KEY = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';
            process.env.COINGECKO_API_KEY = 'test-api-key-123';

            const config = loadConfig();

            const coingeckoProvider = config.providers.find(p => p.name === 'coingecko');
            expect(coingeckoProvider?.baseUrl).toBe('https://pro-api.coingecko.com/api/v3');
            expect(coingeckoProvider?.apiKey).toBe('test-api-key-123');
            expect(coingeckoProvider?.rateLimit.maxRequests).toBe(500);
        });

        it('should include Binance provider configuration', () => {
            process.env.CONTRACT_ID = 'CTEST123456789';
            process.env.ADMIN_SECRET_KEY = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';

            const config = loadConfig();

            const binanceProvider = config.providers.find(p => p.name === 'binance');
            expect(binanceProvider).toBeDefined();
            expect(binanceProvider?.enabled).toBe(true);
            expect(binanceProvider?.priority).toBe(3);
            expect(binanceProvider?.baseUrl).toBe('https://api.binance.com/api/v3');
        });

        it('should enable CoinMarketCap provider when API key is provided', () => {
            process.env.CONTRACT_ID = 'CTEST123456789';
            process.env.ADMIN_SECRET_KEY = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';
            process.env.COINMARKETCAP_API_KEY = 'cmc-test-key';

            const config = loadConfig();

            const cmcProvider = config.providers.find(p => p.name === 'coinmarketcap');
            expect(cmcProvider?.enabled).toBe(true);
            expect(cmcProvider?.apiKey).toBe('cmc-test-key');
        });

        it('should disable CoinMarketCap provider when no API key', () => {
            process.env.CONTRACT_ID = 'CTEST123456789';
            process.env.ADMIN_SECRET_KEY = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';

            const config = loadConfig();

            const cmcProvider = config.providers.find(p => p.name === 'coinmarketcap');
            expect(cmcProvider?.enabled).toBe(false);
        });

        it('should accept valid STELLAR_RPC_URL', () => {
            process.env.CONTRACT_ID = 'CTEST123456789';
            process.env.ADMIN_SECRET_KEY = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';
            process.env.STELLAR_RPC_URL = 'https://custom-rpc.stellar.org';

            const config = loadConfig();

            expect(config.stellarRpcUrl).toBe('https://custom-rpc.stellar.org');
        });

        it('should handle log level validation', () => {
            process.env.CONTRACT_ID = 'CTEST123456789';
            process.env.ADMIN_SECRET_KEY = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';

            const logLevels = ['debug', 'info', 'warn', 'error'] as const;

            logLevels.forEach(level => {
                process.env.LOG_LEVEL = level;
                const config = loadConfig();
                expect(config.logLevel).toBe(level);
            });
        });
    });

    describe('Asset Mappings', () => {
        it('should have mappings for all supported assets', () => {
            expect(ASSET_MAPPINGS.length).toBeGreaterThan(0);

            const expectedAssets = ['XLM', 'USDC', 'USDT', 'BTC', 'ETH'];
            const mappedAssets = ASSET_MAPPINGS.map(m => m.symbol);

            expectedAssets.forEach(asset => {
                expect(mappedAssets).toContain(asset);
            });
        });

        it('should have valid CoinGecko IDs for all assets', () => {
            ASSET_MAPPINGS.forEach(mapping => {
                expect(mapping.coingeckoId).toBeDefined();
                expect(mapping.coingeckoId.length).toBeGreaterThan(0);
            });
        });

        it('should have valid Binance symbols for all assets', () => {
            ASSET_MAPPINGS.forEach(mapping => {
                expect(mapping.binanceSymbol).toBeDefined();
                expect(mapping.binanceSymbol.length).toBeGreaterThan(0);
                // Most assets paired with USDT, but USDT itself uses BUSD
                expect(mapping.binanceSymbol).toMatch(/(USDT|BUSD)$/);
            });
        });

        it('should have valid CoinMarketCap IDs for all assets', () => {
            ASSET_MAPPINGS.forEach(mapping => {
                expect(mapping.coinmarketcapId).toBeDefined();
                expect(mapping.coinmarketcapId).toBeGreaterThan(0);
            });
        });
    });

    describe('getAssetMapping', () => {
        it('should return correct mapping for XLM', () => {
            const mapping = getAssetMapping('XLM');

            expect(mapping).toBeDefined();
            expect(mapping?.symbol).toBe('XLM');
            expect(mapping?.coingeckoId).toBe('stellar');
            expect(mapping?.binanceSymbol).toBe('XLMUSDT');
        });

        it('should return correct mapping for BTC', () => {
            const mapping = getAssetMapping('BTC');

            expect(mapping).toBeDefined();
            expect(mapping?.symbol).toBe('BTC');
            expect(mapping?.coingeckoId).toBe('bitcoin');
            expect(mapping?.binanceSymbol).toBe('BTCUSDT');
        });

        it('should return correct mapping for ETH', () => {
            const mapping = getAssetMapping('ETH');

            expect(mapping).toBeDefined();
            expect(mapping?.symbol).toBe('ETH');
            expect(mapping?.coingeckoId).toBe('ethereum');
            expect(mapping?.binanceSymbol).toBe('ETHUSDT');
        });

        it('should return correct mapping for USDC', () => {
            const mapping = getAssetMapping('USDC');

            expect(mapping).toBeDefined();
            expect(mapping?.symbol).toBe('USDC');
            expect(mapping?.coingeckoId).toBe('usd-coin');
        });

        it('should return undefined for unsupported asset', () => {
            // @ts-ignore - Testing runtime behavior
            const mapping = getAssetMapping('UNKNOWN');

            expect(mapping).toBeUndefined();
        });
    });

    describe('isSupportedAsset', () => {
        it('should return true for XLM', () => {
            expect(isSupportedAsset('XLM')).toBe(true);
        });

        it('should return true for BTC', () => {
            expect(isSupportedAsset('BTC')).toBe(true);
        });

        it('should return true for ETH', () => {
            expect(isSupportedAsset('ETH')).toBe(true);
        });

        it('should return true for USDC', () => {
            expect(isSupportedAsset('USDC')).toBe(true);
        });

        it('should return true for USDT', () => {
            expect(isSupportedAsset('USDT')).toBe(true);
        });

        it('should return false for unsupported asset', () => {
            expect(isSupportedAsset('UNKNOWN')).toBe(false);
            expect(isSupportedAsset('DOGE')).toBe(false);
            expect(isSupportedAsset('SOL')).toBe(false);
        });

        it('should return false for empty string', () => {
            expect(isSupportedAsset('')).toBe(false);
        });

        it('should be case-sensitive', () => {
            expect(isSupportedAsset('xlm')).toBe(false);
            expect(isSupportedAsset('btc')).toBe(false);
        });
    });

    describe('Price Scaling', () => {
        it('should scale price correctly', () => {
            expect(scalePrice(1)).toBe(1_000_000n);
            expect(scalePrice(0.15)).toBe(150_000n);
            expect(scalePrice(50000)).toBe(50_000_000_000n);
        });

        it('should handle decimal prices', () => {
            expect(scalePrice(0.123456)).toBe(123_456n);
            expect(scalePrice(1.5)).toBe(1_500_000n);
            expect(scalePrice(123.456789)).toBe(123_456_789n);
        });

        it('should handle very small prices', () => {
            expect(scalePrice(0.000001)).toBe(1n);
            expect(scalePrice(0.0000015)).toBe(2n); // Rounded
        });

        it('should handle large prices', () => {
            expect(scalePrice(100000)).toBe(100_000_000_000n);
            expect(scalePrice(1000000)).toBe(1_000_000_000_000n);
        });

        it('should handle zero', () => {
            expect(scalePrice(0)).toBe(0n);
        });

        it('should round to nearest integer', () => {
            expect(scalePrice(0.1234567)).toBe(123_457n); // Rounds up
            expect(scalePrice(0.1234564)).toBe(123_456n); // Rounds down
        });
    });

    describe('Price Unscaling', () => {
        it('should unscale price correctly', () => {
            expect(unscalePrice(1_000_000n)).toBe(1);
            expect(unscalePrice(150_000n)).toBe(0.15);
            expect(unscalePrice(50_000_000_000n)).toBe(50000);
        });

        it('should handle decimal results', () => {
            expect(unscalePrice(123_456n)).toBe(0.123456);
            expect(unscalePrice(1_500_000n)).toBe(1.5);
        });

        it('should handle zero', () => {
            expect(unscalePrice(0n)).toBe(0);
        });

        it('should handle large values', () => {
            expect(unscalePrice(100_000_000_000n)).toBe(100000);
            expect(unscalePrice(1_000_000_000_000n)).toBe(1000000);
        });

        it('should be inverse of scalePrice', () => {
            const testPrices = [0.15, 1.5, 50000, 0.000001, 100000];

            testPrices.forEach(price => {
                const scaled = scalePrice(price);
                const unscaled = unscalePrice(scaled);
                expect(unscaled).toBeCloseTo(price, 6);
            });
        });
    });

    describe('PRICE_SCALE constant', () => {
        it('should be defined as 1,000,000', () => {
            expect(PRICE_SCALE).toBe(1_000_000n);
        });

        it('should be a bigint', () => {
            expect(typeof PRICE_SCALE).toBe('bigint');
        });
    });
});
