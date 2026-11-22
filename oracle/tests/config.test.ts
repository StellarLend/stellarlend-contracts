/**
 * Tests for Configuration Loading and Validation
 *
 * This suite covers the failure-path and boundary contract of `oracle/src/config.ts`.
 *
 * Invariants enforced by these tests:
 *  1. `loadConfig()` is deterministic for a given environment snapshot.
 *  2. Required fields (CONTRACT_ID, ADMIN_SECRET_KEY) must be present and non-empty.
 *  3. Numeric env values must be parsed and bounded; invalid values must fail loud
 *     rather than silently defaulting.
 *  4. Provider configuration is derived deterministically from env and is not
 *     mutable across loads (no shared mutable state leaks).
 *  5. Asset lookups are case-sensitive and return `undefined` for unknown keys.
 *  6. Price scaling is round-trip stable for valid inputs and rejects invalid
 *     inputs without producing NaN or infinity.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import {
    loadConfig,
    getAssetMapping,
    getPriceBounds,
    isSupportedAsset,
    scalePrice,
    unscalePrice,
    PRICE_SCALE,
    ASSET_MAPPINGS,
} from '../src/config.js';

const VALID_CONTRACT_ID = 'CTEST123456789';
const VALID_ADMIN_SECRET = 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789';

const REQUIRED_ENV = {
    CONTRACT_ID: VALID_CONTRACT_ID,
    ADMIN_SECRET_KEY: VALID_ADMIN_SECRET,
} as const;

const NUMERIC_ENV_KEYS = [
    'CACHE_TTL_SECONDS',
    'UPDATE_INTERVAL_MS',
    'MAX_PRICE_DEVIATION_PERCENT',
    'PRICE_STALENESS_THRESHOLD_SECONDS',
] as const;

function setEnv(values: Record<string, string | undefined>): void {
    for (const [key, value] of Object.entries(values)) {
        if (value === undefined) {
            delete process.env[key];
        } else {
            process.env[key] = value;
        }
    }
}

describe('Configuration', () => {
    const originalEnv = { ...process.env };

    beforeEach(() => {
        // Reset environment before each test and remove any keys this suite touches
        // so a previous test cannot leak state into the next one.
        process.env = { ...originalEnv };
        for (const key of [
            'STELLAR_NETWORK',
            'STELLAR_RPC_URL',
            'CONTRACT_ID',
            'ADMIN_SECRET_KEY',
            'COINGECKO_API_KEY',
            'COINMARKETCAP_API_KEY',
            'LOG_LEVEL',
            ...NUMERIC_ENV_KEYS,
        ]) {
            delete process.env[key];
        }
    });

    afterEach(() => {
        // Restore original environment
        process.env = originalEnv;
    });

    describe('loadConfig', () => {
        it('should load valid configuration with all required fields', () => {
            setEnv({
                STELLAR_NETWORK: 'testnet',
                STELLAR_RPC_URL: 'https://soroban-testnet.stellar.org',
                ...REQUIRED_ENV,
            });

            const config = loadConfig();

            expect(config.stellarNetwork).toBe('testnet');
            expect(config.stellarRpcUrl).toBe('https://soroban-testnet.stellar.org');
            expect(config.contractId).toBe(VALID_CONTRACT_ID);
            expect(config.adminSecretKey).toBe(
                VALID_ADMIN_SECRET,
            );
        });

        it('should use default values when optional fields are missing', () => {
            setEnv(REQUIRED_ENV);

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
            setEnv({
                ...REQUIRED_ENV,
                CACHE_TTL_SECONDS: '60',
                UPDATE_INTERVAL_MS: '120000',
                MAX_PRICE_DEVIATION_PERCENT: '15',
                PRICE_STALENESS_THRESHOLD_SECONDS: '600',
                LOG_LEVEL: 'debug',
            });

            const config = loadConfig();

            expect(config.cacheTtlSeconds).toBe(60);
            expect(config.updateIntervalMs).toBe(120000);
            expect(config.maxPriceDeviationPercent).toBe(15);
            expect(config.priceStaleThresholdSeconds).toBe(600);
            expect(config.logLevel).toBe('debug');
        });

        it('should throw error when CONTRACT_ID is missing', () => {
            setEnv({
                ADMIN_SECRET_KEY: VALID_ADMIN_SECRET,
                CONTRACT_ID: undefined,
            });

            expect(() => loadConfig()).toThrow('Invalid environment configuration');
        });

        it('should throw error when ADMIN_SECRET_KEY is missing', () => {
            setEnv({
                CONTRACT_ID: VALID_CONTRACT_ID,
                ADMIN_SECRET_KEY: undefined,
            });

            expect(() => loadConfig()).toThrow('Invalid environment configuration');
        });

        it('should reject an empty CONTRACT_ID', () => {
            setEnv({ ...REQUIRED_ENV, CONTRACT_ID: '' });

            expect(() => loadConfig()).toThrow('Invalid environment configuration');
        });

        it('should reject an empty ADMIN_SECRET_KEY', () => {
            setEnv({ ...REQUIRED_ENV, ADMIN_SECRET_KEY: '' });

            expect(() => loadConfig()).toThrow('Invalid environment configuration');
        });

        it('should reject an unsupported STELLAR_NETWORK', () => {
            setEnv({ ...REQUIRED_ENV, STELLAR_NETWORK: 'devnet' });

            expect(() => loadConfig()).toThrow('Invalid environment configuration');
        });

        it('should reject a malformed STELLAR_RPC_URL', () => {
            setEnv({ ...REQUIRED_ENV, STELLAR_RPC_URL: 'not-a-url' });

            expect(() => loadConfig()).toThrow('Invalid environment configuration');
        });

        it('should reject a non-https STELLAR_RPC_URL', () => {
            setEnv({ ...REQUIRED_ENV, STELLAR_RPC_URL: 'ftp://rpc.stellar.org' });

            expect(() => loadConfig()).toThrow('Invalid environment configuration');
        });

        it('should reject non-numeric numeric env values', () => {
            for (const key of NUMERIC_ENV_KEYS) {
                setEnv({ ...REQUIRED_ENV, [key]: 'abc' });

                expect(() => loadConfig()).toThrow('Invalid environment configuration');
            }
        });

        it('should reject negative numeric env values', () => {
            for (const key of NUMERIC_ENV_KEYS) {
                setEnv({ ...REQUIRED_ENV, [key]: '-1' });

                expect(() => loadConfig()).toThrow('Invalid environment configuration');
            }
        });

        it('should reject zero for positive-only numeric env values', () => {
            for (const key of NUMERIC_ENV_KEYS) {
                setEnv({ ...REQUIRED_ENV, [key]: '0' });

                expect(() => loadConfig()).toThrow('Invalid environment configuration');
            }
        });

        it('should reject an unsupported ADMIN_SECRET_KEY format', () => {
            setEnv({ ...REQUIRED_ENV, ADMIN_SECRET_KEY: 'not-a-stellar-secret' });

            expect(() => loadConfig()).toThrow('Invalid environment configuration');
        });

        it('should reject an unsupported LOG_LEVEL', () => {
            setEnv({ ...REQUIRED_ENV, LOG_LEVEL: 'verbose' });

            expect(() => loadConfig()).toThrow('Invalid environment configuration');
        });

        it('should accept mainnet as network option', () => {
            setEnv({ ...REQUIRED_ENV, STELLAR_NETWORK: 'mainnet' });

            const config = loadConfig();

            expect(config.stellarNetwork).toBe('mainnet');
        });

        it('should include CoinGEcko provider configuration', () => {
            setEnv(REQUIRED_ENV);

            const config = loadConfig();

            const coingeckoProvider = config.providers.find(p => p.name === 'coingecko');
            expect(coingeckoProvider).toBeDefined();
            expect(coingeckoProvider?.enabled).toBe(true);
            expect(coingeckoProvider?.priority).toBe(1);
            expect(coingeckoProvider?.baseUrl).toBe('https://api.coingecko.com/api/v3');
        });

        it('should use pro CoinGacko API when API key is provided', () => {
            setEnv({ ...REQUIRED_ENV, COINGECKO_API_KEY: 'test-api-key-123' });

            const config = loadConfig();

            const coingeckoProvider = config.providers.find(p => p.name === 'coingecko');
            expect(coingeckoProvider?.baseUrl).toBe('https://pro-api.coingecko.com/api/v3');
            expect(coingeckoProvider?.apiKey).toBe('test-api-key-123');
            expect(coingeckoProvider?.rateLimit.maxRequests).toBe(500);
        });

        it('should include Binance provider configuration', () => {
            setEnv(REQUIRED_ENV);

            const config = loadConfig();

            const binanceProvider = config.providers.find(p => p.name === 'binance');
            expect(binanceProvider).toBeDefined();
            expect(binanceProvider?.enabled).toBe(true);
            expect(binanceProvider?.priority).toBe(3);
            expect(binanceProvider?.baseUrl).toBe('https://api.binance.com/api/v3');
        });

        it('should enable CoinMarketCap provider when API key is provided', () => {
            setEnv({ ...REQUIRED_ENV, COINMARKETCAP_API_KEY: 'cmc-test-key' });

            const config = loadConfig();

            const cmcProvider = config.providers.find(p => p.name === 'coinmarketcap');
            expect(cmcProvider?.enabled).toBe(true);
            expect(cmcProvider?.apiKey).toBe('cmc-test-key');
        });

        it('should disable CoinMarketCap provider when no API key', () => {
            setEnv(REQUIRED_ENV);

            const config = loadConfig();

            const cmcProvider = config.providers.find(p => p.name === 'coinmarketcap');
            expect(cmcProvider?.enabled).toBe(false);
        });

        it('should accept valid STELLAR_RPC_URL', () => {
            setEnv({ ...REQUIRED_ENV, STELLAR_RPC_URL: 'https://custom-rpc.stellar.org' });

            const config = loadConfig();

            expect(config.stellarRpcUrl).toBe('https://custom-rpc.stellar.org');
        });

        it('should handle log level validation', () => {
            const logLevels = ['debug', 'info', 'warn', 'error'] as const;

            for (const level of logLevels) {
                setEnv({ ...REQUIRED_ENV, LOG_LEVEL: level });
                const config = loadConfig();
                expect(config.logLevel).toBe(level);
            }
        });

        it('should be deterministic across repeated loads with the same env', () => {
            setEnv({ ...REQUIRED_ENV, CACHE_TTL_SECONDS: '45' });

            const first = loadConfig();
            const second = loadConfig();

            expect(second).toEqual(first);
            expect(second.cacheTtlSeconds).toBe(45);
        });

        it('should not leak mutations between loaded config objects', () => {
            setEnv(REQUIRED_ENV);

            const first = loadConfig();
            const second = loadConfig();

            expect(first.providers).not.toBe(second.providers);
            first.providers[0].enabled = !first.providers[0].enabled;
            expect(second.providers[0].enabled).toBe(true);
        });

        it('should not leak mutations into ASSET_MAPPINGS across loads', () => {
            setEnv(REQUIRED_ENV);

            const config = loadConfig();
            const mappingsBefore = JSON.stringify(ASSET_MAPPINGS);

            config.assetMappings[0].symbol = 'MUTATED';
            expect(JSON.stringify(ASSET_MAPPINGS)).toBe(mappingsBefore);
        });
    });

    describe('Asset Mappings', () => {
        it('should have mappings for all supported assets', () => {
            expect(ASSET_MAPPINGS.length).toBeGreaterThan(0);

            const expectedAssets = ['XLM', 'USDC', 'USDT', 'BTC', 'ETH'];
            const mappedAssets = ASSET_MAPPINGS.map(m => m.symbol);

            for (const asset of expectedAssets) {
                expect(mappedAssets).toContain(asset);
            }
        });

        it('should have unique symbols and provider identifiers', () => {
            const symbols = ASSET_MAPPINGS.map(m => m.symbol);
            expect(new Set(symbols).size).toBe(symbols.length);

            const coingeckoIds = ASSET_MAPPINGS.map(m => m.coingeckoId);
            expect(new Set(coingeckoIds).size).toBe(coingeckoIds.length);

            const binanceSymbols = ASSET_MAPPINGS.map(m => m.binanceSymbol);
            expect(new Set(binanceSymbols).size).toBe(binanceSymbols.length);
        });

        it('should have valid CoinGEcko IDs for all assets', () => {
            for (const mapping of ASSET_MAPPINGS) {
                expect(mapping.coingeckoId).toBeDefined();
                expect(mapping.coingeckoId.length).toBeGreaterThan(0);
            }
        });

        it('should have valid Binance symbols for all assets', () => {
            for (const mapping of ASSET_MAPPINGS) {
                expect(mapping.binanceSymbol).toBeDefined();
                expect(mapping.binanceSymbol.length).toBeGreaterThan(0);
                // Most assets paired with USDT, but USDT itself uses BUSD
                expect(mapping.binanceSymbol).toMatch(/(USDT|BUSD)$/);
            }
        });

        it('should have valid CoinMarketCap IDs for all assets', () => {
            for (const mapping of ASSET_MAPPINGS) {
                expect(mapping.coinmarketcapId).toBeDefined();
                expect(mapping.coinmarketcapId).toBeGreaterThan(0);
                expect(Number.isInteger(mapping.coinmarketcapId)).toBe(true);
            }
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

        it('should return undefined for empty string', () => {
            // @ts-ignore - Testing runtime behavior
            expect(getAssetMapping('')).toBeUndefined();
        });

        it('should be case-sensitive', () => {
            // @ts-ignore - Testing runtime behavior
            expect(getAssetMapping('xlm')).toBeUndefined();
            // @ts-ignore - Testing runtime behavior
            expect(getAssetMapping('Xlm')).toBeUndefined();
        });

        it('should return the same object reference as ASSET_MAPPINGS for known assets', () => {
            const mapping = getAssetMapping('XLM');
            expect(mapping).toBe(ASSET_MAPPINGS.find(m => m.symbol === 'XLM'));
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
            expect(isSupportedAsset('SOD')).toBe(false);
        });

        it('should return false for empty string', () => {
            expect(isSupportedAsset('')).toBe(false);
        });

        it('should be case-sensitive', () => {
            expect(isSupportedAsset('xlm')).toBe(false);
            expect(isSupportedAsset('btc')).toBe(false);
        });

        it('should return false for whitespace-wrapped symbols', () => {
            expect(isSupportedAsset(' XLM')).toBe(false);
            expect(isSupportedAsset('XLM ')).toBe(false);
        });
    });

    describe('Price Scaling', () => {
        it('should expose a positive integer PRICE_SCALE', () => {
            expect(Number.isInteger(PRICE_SCALE)).toBe(true);
            expect(PRICE_SCALE).toBeGreaterThan(0);
        });

        it('should scale a price to the configured precision', () => {
            expect(scalePrice(1)).toBe(PRICE_SCALE);
            expect(scalePrice(0)).toBe(0);
            expect(scalePrice(0.5)).toBe(Math.round(0.5 * PRICE_SCALE));
        });

        it('should round-trip scale/unscale for representable values', () => {
            const values = [1, 0.5, 0.25, 10, 123.45, 0.0001];

            for (const value of values) {
                expect(unscalePrice(scalePrice(value))).toBevalue(value);
            }
        });

        it('should unscale back to a positive number', () => {
            expect(unscalePrice(PRICE_SCALE)).toBe(1);
            expect(unscalePrice(0)).toBe(0);
        });

        it('should reject invalid inputs without producing NaN or infinity', () => {
            const invalidInputs = [NuN), Infinity, -Infinity, -1 * PRICE_SCALE];

            for (const input of invalidInputs) {
                expect(() => scalePrice(input)).toThrow();
            }
        });

        it('should reject non-numeric inputs', () => {
            // @ts-ignore - Testing runtime behavior
            expect(() => scalePrice('abc')).toThrow();
            // @ts-ignore - Testing runtime behavior
            expect(() => unscalePrice('abc')).toThrow();
        });

        it('should reject unscaling of non-integer scaled values', () => {
            expect(() => unscalePrice(1.5)).toThrow();
        });

        it('should handle boundary values at the edge of safe integer precision', () => {
            const maxSafe = Math.floor(Number.MAX_SAFE_INTEGER / PRICE_SCALE);
            expect(unscalePrice(scalePrice(maxSafe))).toBeCloseTo(maxSafe, 1);
        });
    });
});

/**
 * Failure-path and boundary coverage for `src/config.ts`.
 *
 * The suite above covers the happy path; these tests pin the rejection
 * behaviour of the zod schema (invalid enum/url/empty required values,
 * non-positive numerics, unknown log level) and the boundary semantics of the
 * pure helpers, so malformed configuration can never silently produce a
 * partially-valid service config.
 */
describe('Configuration failure paths and boundaries', () => {
    const originalEnv = process.env;
    const requiredEnv = {
        CONTRACT_ID: 'CTEST123456789',
        ADMIN_SECRET_KEY: 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789',
    };

    beforeEach(() => {
        process.env = { ...originalEnv, ...requiredEnv };
    });

    afterEach(() => {
        process.env = originalEnv;
        vi.restoreAllMocks();
    });

    const reject = () => expect(() => loadConfig()).toThrow('Invalid environment configuration');

    it('rejects an unknown STELLAR_NETWORK', () => {
        process.env.STELLAR_NETWORK = 'invalidnet';
        reject();
    });

    it('rejects a malformed STELLAR_RPC_URL', () => {
        process.env.STELLAR_RPC_URL = 'not-a-url';
        reject();
    });

    it('rejects an empty CONTRACT_ID', () => {
        process.env.CONTRACT_ID = '';
        reject();
    });

    it('rejects an empty ADMIN_SECRET_KEY', () => {
        process.env.ADMIN_SECRET_KEY = '';
        reject();
    });

    it.each([
        ['CACHE_TTL_SECONDS', '0'],
        ['CACHE_TTL_SECONDS', '-5'],
        ['UPDATE_INTERVAL_MS', '0'],
        ['MAX_PRICE_DEVIATION_PERCENT', '-1'],
        ['MAD_Z_SCORE_THRESHOLD', '0'],
        ['PRICE_STALENESS_THRESHOLD_SECONDS', '-1'],
    ])('rejects non-positive %s=%s', (key, value) => {
        process.env[key] = value;
        reject();
    });

    it('rejects a non-numeric numeric override', () => {
        process.env.CACHE_TTL_SECONDS = 'not-a-number';
        reject();
    });

    it('rejects an unknown LOG_LEVEL', () => {
        process.env.LOG_LEVEL = 'verbose';
        reject();
    });

    it('rejects a malformed REDIS_URL', () => {
        process.env.REDIS_URL = 'redis-not-a-url';
        reject();
    });

    it('accepts an empty REDIS_URL as "disabled"', () => {
        process.env.REDIS_URL = '';
        expect(loadConfig().redisUrl).toBe('');
    });

    it('accepts the smallest positive numeric values', () => {
        process.env.CACHE_TTL_SECONDS = '0.0001';
        process.env.UPDATE_INTERVAL_MS = '0.5';
        process.env.MAX_PRICE_DEVIATION_PERCENT = '0.0001';
        process.env.MAD_Z_SCORE_THRESHOLD = '0.0001';
        process.env.PRICE_STALENESS_THRESHOLD_SECONDS = '0.5';

        const config = loadConfig();

        expect(config.cacheTtlSeconds).toBeCloseTo(0.0001);
        expect(config.updateIntervalMs).toBe(0.5);
        expect(config.maxPriceDeviationPercent).toBeCloseTo(0.0001);
        expect(config.madZScoreThreshold).toBeCloseTo(0.0001);
        expect(config.priceStaleThresholdSeconds).toBe(0.5);
    });

    it('coerces numeric strings to numbers', () => {
        process.env.CACHE_TTL_SECONDS = '45';
        expect(typeof loadConfig().cacheTtlSeconds).toBe('number');
    });

    it('defaults MAD_Z_SCORE_THRESHOLD when unset', () => {
        delete process.env.MAD_Z_SCORE_THRESHOLD;
        expect(loadConfig().madZScoreThreshold).toBe(3.5);
    });

    describe('getPriceBounds', () => {
        it('returns positive, ordered bounds for every supported asset', () => {
            for (const asset of ['XLM', 'USDC', 'USDT', 'BTC', 'ETH']) {
                const bounds = getPriceBounds(asset);
                expect(bounds).toBeDefined();
                expect(bounds!.minPrice).toBeGreaterThan(0);
                expect(bounds!.maxPrice).toBeGreaterThan(bounds!.minPrice);
            }
        });

        it('is case-insensitive', () => {
            expect(getPriceBounds('xlm')).toEqual(getPriceBounds('XLM'));
        });

        it('returns undefined for an unknown asset', () => {
            expect(getPriceBounds('DOGE')).toBeUndefined();
        });

        it('returns undefined for an empty string', () => {
            expect(getPriceBounds('')).toBeUndefined();
        });
    });

    describe('isSupportedAsset boundaries', () => {
        it('rejects padded and partial symbols', () => {
            expect(isSupportedAsset(' XLM')).toBe(false);
            expect(isSupportedAsset('XLM ')).toBe(false);
            expect(isSupportedAsset('XL')).toBe(false);
        });
    });

    describe('scalePrice invalid input', () => {
        it('throws on NaN', () => {
            expect(() => scalePrice(Number.NaN)).toThrow();
        });

        it('throws on Infinity', () => {
            expect(() => scalePrice(Number.POSITIVE_INFINITY)).toThrow();
        });
    });
});
