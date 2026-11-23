/**
 * Oracle Service Configuration
 *
 * Handles loading and validating environment variables and
 * provides typed configuration for the oracle service.
 *
 * Invariants enforced by this module:
 *   - Configuration is deterministic and validated before use.
 *   - Invalid or missing required values fail fast with diagnosable errors.
 *   - Numeric boundaries (positive, finite, within safe integer range) are enforced.
 *   - Provider configuration is derived deterministically from validated env.
 *   - Secrets are never logged or included in error messages.
 */

import { z } from 'zod';
import dotenv from 'dotenv';
import { StrKey } from '@stellar/stellar-sdk';
import type { OracleServiceConfig, ProviderConfig, AssetMapping, SupportedAsset } from './types/index.js';

export type { OracleServiceConfig } from './types/index.js';

dotenv.config();

/**
 * Maximum safe integer for JS numeric operations.
 * Values above this can lose precision and must be rejected.
 */
const MAX_SAFE_INTEGER = Number.MAX_SAFE_INTEGER;

/**
 * Rejects non-finite, non-positive, or unsafe numeric values.
 * This guards against NaN, Infinity, negative values, and precision loss.
 */
export const positiveFiniteNumber = (label: string) =>
    z.coerce.number().refine((n) => Number.isFinite(n), { message: `${label} must be a finite number` }).refine((n) => n > 0, { message: `${label} must be greater than 0` }).refine((n) => Number.isInteger(n) ? n <= MAX_SAFE_INTEGER : true, { message: `${label} exceeds maximum safe integer` });

/**
 * Environment variable validation schema
 *
 * Note: we do not echo values for secret fields in error messages.
 */
const envSchema = z.object({
    STELLAR_NETWORK: z.enum(['testnet', 'mainnet']).default('testnet'),
    STELLAR_RPC_URL: z.string().url().default('https://soroban-testnet.stellar.org'),
    CONTRACT_ID: z.string().min(1, 'CONTRACT_ID is required').refine(
        (value) => StrKey.isValidContract(value),
        { message: 'CONTRACT_ID must be a valid Stellar contract ID' },
    ),
    ADMIN_SECRET_KEY: z.string().min(1, 'ADMIN_SECRET_KEY is required').refine(
        (value) => StrKey.isValidEd25519SecretSeed(value),
        { message: 'ADMIN_SECRET_KEY must be a valid Stellar secret seed' },
    ),
    COINGECKO_API_KEY: z.string().optional(),
    COINMARKETCAP_API_KEY: z.string().optional(),
    REDIS_URL: z.string().url().optional().or(z.literal('')),
    CACHE_TTL_SECONDS: positiveFiniteNumber('CACHE_TTL_SECONDS').default(30),
    UPDATE_INTERVAL_MS: positiveFiniteNumber('UPDATE_INTERVAL_MS').default(60000),
    MAX_PRICE_DEVIATION_PERCENT: positiveFiniteNumber('MAX_PRICE_DEVIATION_PERCENT').default(10),
    MAD_Z_SCORE_THRESHOLD: positiveFiniteNumber('MAD_Z_SCORE_THRESHOLD').default(3.5),
    PRICE_STALENESS_THRESHOLD_SECONDS: positiveFiniteNumber('PRICE_STALENESS_THRESHOLD_SECONDS').default(300),
    LOG_LEVEL: z.enum(['debug', 'info', 'warn', 'error']).default('info'),
});

/**
 * Parse and validate environment variables.
 *
 * This function is deterministic and fails fast on invalid input.
 * It never logs secret values - only the field name and validation message.
 */
export function parseEnv(env: NodeJS.ProcessEnv = process.env) {
    const result = envSchema.safeParse(env);

    if (!result.success) {
        const issues = result.error.issues.map(
            (issue) => `${issue.path.join('.')}: ${issue.message}`,
        );
        console.error('❌ Environment validation failed:');
        issues.forEach((line) => console.error(`\t- ${line}`));
        const error = new Error('Invalid environment configuration');
        // Attach machine-readable issues for testing and observability.
        (error as Error & { code?: string; issues?: string[] }).code = 'ENV_VALIDATION_FAILED';
        (error as Error & { code?: string; issues?: string[] }).issues = issues;
        throw error;
    }

    return result.data;
}

/**
 * Default provider configurations.
 *
 * Provider ordering and weights are deterministic and stable.
 * Only the coinmarketcap provider is conditionally enabled based on key presence.
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
 * Asset mappings for different providers.
 *
 * Invariant: symbols are unique and non-empty.
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
 * Get asset mapping by symbol.
 * Returns undefined for unknown or non-string inputs.
 */
export function getAssetMapping(symbol: SupportedAsset): AssetMapping | undefined {
    if (typeof symbol !== 'string') return undefined;
    return ASSET_MAPPINGS.find((m) => m.symbol === symbol);
}

/**
 * Check if an asset is supported.
 * Type guard that is safe for arbitrary string inputs.
 */
export function isSupportedAsset(symbol: string): symbol is SupportedAsset {
    if (typeof symbol !== 'string' || symbol.length === 0) return false;
    return ASSET_MAPPINGS.some((m) => m.symbol === symbol);
}

/**
 * Build and export the service configuration.
 *
 * This is the primary entry point for consumers. It validates env,
 * derives provider configs, and returns a typed config object.
 * It is pure with respect to its inputs (except for the default env).
 */
export function loadConfig(env: NodeJS.ProcessEnv = process.env): OracleServiceConfig {
    const parsed = parseEnv(env);

    return {
        stellarNetwork: parsed.STELLAR_NETWORK,
        stellarRpcUrl: parsed.STELLAR_RPC_URL,
        contractId: parsed.CONTRACT_ID,
        adminSecretKey: parsed.ADMIN_SECRET_KEY,
        updateIntervalMs: parsed.UPDATE_INTERVAL_MS,
        maxPriceDeviationPercent: parsed.MAX_PRICE_DEVIATION_PERCENT,
        madZScoreThreshold: parsed.MAD_Z_SCORE_THRESHOLD,
        priceStaleThresholdSeconds: parsed.PRICE_STALENESS_THRESHOLD_SECONDS,
        cacheTtlSeconds: parsed.CACHE_TTL_SECONDS,
        redisUrl: parsed.REDIS_URL,
        logLevel: parsed.LOG_LEVEL,
        providers: getProviderConfigs(parsed),
    };
}

export const PRICE_SCALE = 1_000_000n;

/**
 * Scale a decimal string to a 6-decimal bigint using exact decimal-string arithmetic.
 * Half-up rounding is applied to the 7th decimal digit.
 */
export function scaleDecimalString(str: string): bigint {
    const trimmed = str.trim();
    if (!trimmed || trimmed === '.' || trimmed === '-' || trimmed === '+') {
        throw new Error(`scalePrice: invalid decimal string '${str}'`);
    }
    const isNegative = trimmed.startsWith('-');
    const withoutSign = (trimmed.startsWith('-') || trimmed.startsWith('+'))
        ? trimmed.slice(1)
        : trimmed;

    if (!/^\d*(\.\d*)?$/.test(withoutSign) || withoutSign === '' || withoutSign === '.') {
        throw new Error(`scalePrice: invalid decimal string '${str}'`);
    }

    const [intPart = '0', fracPart = ''] = withoutSign.split('.');
    const cleanInt = intPart === '' ? '0' : intPart;

    const paddedFrac = fracPart.padEnd(7, '0');
    const first6 = paddedFrac.slice(0, 6);
    const seventhDigit = parseInt(paddedFrac.charAt(6), 10);

    let base = BigInt(cleanInt) * PRICE_SCALE + BigInt(first6);
    if (seventhDigit >= 5) {
        base += 1n;
    }
    return isNegative ? -base : base;
}

export function scalePrice(price: number | string): bigint {
    if (typeof price === 'string') {
        return scaleDecimalString(price);
    }
    if (typeof price === 'number') {
        if (!Number.isFinite(price)) {
            throw new Error('scalePrice: price must be a finite number');
        }
        const maxSafePrice = Number(Number.MAX_SAFE_INTEGER) / Number(PRICE_SCALE);
        if (Math.abs(price) > maxSafePrice) {
            throw new Error(`scalePrice: number ${price} exceeds safe integer range for scaling (max safe: ${maxSafePrice})`);
        }
        const str = price.toString();
        if (!str.includes('e') && !str.includes('E')) {
            return scaleDecimalString(str);
        }
        return scaleDecimalString(price.toFixed(12));
    }
    throw new Error('scalePrice: price must be a number or string');
}

export function unscalePrice(price: bigint | number): number {
    if (typeof price === 'bigint') {
        return Number(price) / Number(PRICE_SCALE);
    }
    if (typeof price === 'number') {
        if (!Number.isInteger(price)) {
            throw new Error('unscalePrice: non-integer scaled value');
        }
        return price / Number(PRICE_SCALE);
    }
    throw new Error('unscalePrice: price must be a bigint or number');
}

/**
 * Default MAD z-score threshold for outlier rejection.
 * Overridden at runtime by MAD_Z_SCORE_THRESHOLD RN var .
 */
export const MAD_Z_SCORE_THRESHOLD = 3.5;
