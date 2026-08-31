/**
 * Oracle Service Configuration
 * 
 * Handles loading and validating environment variables and
 * provides typed configuration for the oracle service.
 */

import { z } from 'zod';
import dotenv from 'dotenv';
import type { OracleServiceConfig, ProviderConfig, AssetMapping, SupportedAsset } from './types/index.js';

export type { OracleServiceConfig } from './types/index.js';

dotenv.config();

/**
 * Environment variable validation schema
 */
const envSchema = z.object({
    STELLAR_NETWORK: z.enum(['testnet', 'mainnet']).default('testnet'),
    STELLAR_RPC_URL: z.string().url().default('https://soroban-testnet.stellar.org'),
    CONTRACT_ID: z.string().min(1, 'CONTRACT_ID is required'),
    ADMIN_SECRET_KEY: z.string().min(1, 'ADMIN_SECRET_KEY is required'),
    COINGECKO_API_KEY: z.string().optional(),
    COINMARKETCAP_API_KEY: z.string().optional(),
    REDIS_URL: z.string().url().optional().or(z.literal('')),
    CACHE_TTL_SECONDS: z.coerce.number().positive().default(30),
    UPDATE_INTERVAL_MS: z.coerce.number().positive().default(60000),
    MAX_PRICE_DEVIATION_PERCENT: z.coerce.number().positive().default(10),
    MAD_Z_SCORE_THRESHOLD: z.coerce.number().positive().default(3.5),
    PRICE_STALENESS_THRESHOLD_SECONDS: z.coerce.number().positive().default(300),
    LOG_LEVEL: z.enum(['debug', 'info', 'warn', 'error']).default('info'),
    // Admin API
    ADMIN_API_PORT: z.coerce.number().int().min(0).max(65535).default(0),
    ADMIN_HMAC_SECRET: z.string().optional(),
    // Retry / backoff (used by ContractUpdater)
    MAX_RETRIES: z.coerce.number().int().min(0).max(10).default(3),
    BACKOFF_BASE_MS: z.coerce.number().positive().default(1000),
    BACKOFF_CAP_MS: z.coerce.number().positive().default(30000),
    // Concurrency: max parallel provider fetches per update cycle
    MAX_CONCURRENT_PROVIDERS: z.coerce.number().int().min(1).max(20).default(5),
});

/**
 * Parse and validate environment variables
 */
function parseEnv() {
    const result = envSchema.safeParse(process.env);

    if (!result.success) {
        console.error('❌ Environment validation failed:');
        result.error.issues.forEach((issue) => {
            console.error(`  - ${issue.path.join('.')}: ${issue.message}`);
        });
        throw new Error('Invalid environment configuration');
    }

    return result.data;
}

/**
 * Default provider configurations
 */
function getProviderConfigs(env: z.infer<typeof envSchema>): ProviderConfig[] {
    return [
        {
            name: 'coingecko',
            enabled: true,
            priority: 1,
            weight: 0.4,
            apiKey: env.COINGECKO_API_KEY,
            baseUrl: env.COINGECKO_API_KEY
                ? 'https://pro-api.coingecko.com/api/v3'
                : 'https://api.coingecko.com/api/v3',
            rateLimit: {
                maxRequests: env.COINGECKO_API_KEY ? 500 : 10,
                windowMs: 60000,
            },
        },
        {
            name: 'coinmarketcap',
            enabled: !!env.COINMARKETCAP_API_KEY,
            priority: 2,
            weight: 0.35,
            apiKey: env.COINMARKETCAP_API_KEY,
            baseUrl: 'https://pro-api.coinmarketcap.com/v2',
            rateLimit: {
                maxRequests: 30,
                windowMs: 60000,
            },
        },
        {
            name: 'binance',
            enabled: true,
            priority: 3,
            weight: 0.25,
            baseUrl: 'https://api.binance.com/api/v3',
            rateLimit: {
                maxRequests: 1200,
                windowMs: 60000,
            },
        },
    ];
}

/**
 * Asset mappings for different providers
 */
export const ASSET_MAPPINGS: AssetMapping[] = [
    {
        symbol: 'XLM',
        coingeckoId: 'stellar',
        coinmarketcapId: 512,
        binanceSymbol: 'XLMUSDT',
    },
    {
        symbol: 'USDC',
        coingeckoId: 'usd-coin',
        coinmarketcapId: 3408,
        binanceSymbol: 'USDCUSDT',
    },
    {
        symbol: 'USDT',
        coingeckoId: 'tether',
        coinmarketcapId: 825,
        binanceSymbol: 'USDTBUSD',
    },
    {
        symbol: 'BTC',
        coingeckoId: 'bitcoin',
        coinmarketcapId: 1,
        binanceSymbol: 'BTCUSDT',
    },
    {
        symbol: 'ETH',
        coingeckoId: 'ethereum',
        coinmarketcapId: 1027,
        binanceSymbol: 'ETHUSDT',
    },
];

/**
 * Get asset mapping by symbol
 */
export function getAssetMapping(symbol: SupportedAsset): AssetMapping | undefined {
    return ASSET_MAPPINGS.find((m) => m.symbol === symbol);
}

/**
 * Check if an asset is supported
 */
export function isSupportedAsset(symbol: string): symbol is SupportedAsset {
    return ASSET_MAPPINGS.some((m) => m.symbol === symbol);
}

/**
 * Default per-asset price bounds applied by the validator.
 * These are conservative safe-guards; operators should tune them for their deployment.
 */
export const DEFAULT_PRICE_BOUNDS: Partial<Record<import('./types/index.js').SupportedAsset, import('./types/index.js').AssetPriceBounds>> = {
    XLM:  { minPrice: 0.001,   maxPrice: 100 },
    USDC: { minPrice: 0.9,     maxPrice: 1.1 },
    USDT: { minPrice: 0.9,     maxPrice: 1.1 },
    BTC:  { minPrice: 1_000,   maxPrice: 500_000 },
    ETH:  { minPrice: 50,      maxPrice: 50_000 },
};

/**
 * Build and export the service configuration
 */
export function loadConfig(): OracleServiceConfig {
    const env = parseEnv();

    if (env.ADMIN_API_PORT > 0 && !env.ADMIN_HMAC_SECRET) {
        throw new Error('ADMIN_HMAC_SECRET is required when ADMIN_API_PORT is configured');
    }

    return {
        stellarNetwork: env.STELLAR_NETWORK,
        stellarRpcUrl: env.STELLAR_RPC_URL,
        contractId: env.CONTRACT_ID,
        adminSecretKey: env.ADMIN_SECRET_KEY,
        adminApiPort: env.ADMIN_API_PORT,
        adminHmacSecret: env.ADMIN_HMAC_SECRET,
        updateIntervalMs: env.UPDATE_INTERVAL_MS,
        maxPriceDeviationPercent: env.MAX_PRICE_DEVIATION_PERCENT,
        madZScoreThreshold: env.MAD_Z_SCORE_THRESHOLD,
        priceStaleThresholdSeconds: env.PRICE_STALENESS_THRESHOLD_SECONDS,
        cacheTtlSeconds: env.CACHE_TTL_SECONDS,
        redisUrl: env.REDIS_URL,
        logLevel: env.LOG_LEVEL,
        providers: getProviderConfigs(env),
        priceBounds: DEFAULT_PRICE_BOUNDS,
    };
}

export const PRICE_SCALE = 1_000_000n;

export function scalePrice(price: number): bigint {
    return BigInt(Math.round(price * Number(PRICE_SCALE)));
}

export function unscalePrice(price: bigint): number {
    return Number(price) / Number(PRICE_SCALE);
}

/**
 * Default MAD z-score threshold for outlier rejection.
 * Overridden at runtime by MAD_Z_SCORE_THRESHOLD env var.
 */
export const MAD_Z_SCORE_THRESHOLD = 3.5;

/**
 * Runtime-configurable bounds, exported so `ContractUpdater` and tests can
 * read the same validated values from the environment.
 */
function getEnvNumbers() {
    const parsed = envSchema.safeParse(process.env);
    if (!parsed.success) return { maxRetries: 3, backoffBaseMs: 1000, backoffCapMs: 30000, maxConcurrentProviders: 5 };
    return {
        maxRetries: parsed.data.MAX_RETRIES,
        backoffBaseMs: parsed.data.BACKOFF_BASE_MS,
        backoffCapMs: parsed.data.BACKOFF_CAP_MS,
        maxConcurrentProviders: parsed.data.MAX_CONCURRENT_PROVIDERS,
    };
}

export const runtimeConfig = getEnvNumbers();
