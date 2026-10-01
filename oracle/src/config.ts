/**
 * Oracle Service Configuration
 * 
 * Handles loading and validating environment variables and
 * provides typed configuration for the oracle service.
 */

import { z } from 'zod';
import dotenv from 'dotenv';
import type {
    OracleServiceConfig,
    ProviderConfig,
    AssetMapping,
    SupportedAsset,
} from './types/index.js';

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

export interface AssetPriceBounds {
    minPrice: number;
    maxPrice: number;
}

export const DEFAULT_PRICE_BOUNDS: Record<SupportedAsset, AssetPriceBounds> = {
    XLM: { minPrice: 0.00001, maxPrice: 1000000 },
    USDC: { minPrice: 0.9, maxPrice: 1.1 },
    USDT: { minPrice: 0.9, maxPrice: 1.1 },
    BTC: { minPrice: 1000, maxPrice: 200000 },
    ETH: { minPrice: 100, maxPrice: 20000 },
};

export function getPriceBounds(asset: string): AssetPriceBounds | undefined {
    return DEFAULT_PRICE_BOUNDS[asset.toUpperCase() as SupportedAsset];
}

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
 * Build and export the service configuration
 */
export function loadConfig(): OracleServiceConfig {
    const env = parseEnv();

    return {
        stellarNetwork: env.STELLAR_NETWORK,
        stellarRpcUrl: env.STELLAR_RPC_URL,
        contractId: env.CONTRACT_ID,
        adminSecretKey: env.ADMIN_SECRET_KEY,
        updateIntervalMs: env.UPDATE_INTERVAL_MS,
        maxPriceDeviationPercent: env.MAX_PRICE_DEVIATION_PERCENT,
        madZScoreThreshold: env.MAD_Z_SCORE_THRESHOLD,
        priceStaleThresholdSeconds: env.PRICE_STALENESS_THRESHOLD_SECONDS,
        cacheTtlSeconds: env.CACHE_TTL_SECONDS,
        redisUrl: env.REDIS_URL,
        logLevel: env.LOG_LEVEL,
        providers: getProviderConfigs(env),
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

/* -------------------------------------------------------------------------- */
/* Service configuration validation                                           */
/* -------------------------------------------------------------------------- */

/** Provider implementations the service knows how to construct. */
export const SUPPORTED_PROVIDER_NAMES = ['coingecko', 'binance'] as const;

export type SupportedProviderName = (typeof SUPPORTED_PROVIDER_NAMES)[number];

const NETWORKS = ['testnet', 'mainnet'] as const;
const LOG_LEVELS = ['debug', 'info', 'warn', 'error'] as const;

/** A single configuration problem, addressed by a dotted path. */
export interface ConfigValidationIssue {
    path: string;
    message: string;
}

/**
 * Thrown when a service configuration is rejected.
 *
 * All problems are collected before throwing so a misconfigured deployment can
 * be fixed in one pass rather than one restart per error.
 */
export class ConfigValidationError extends Error {
    readonly issues: ConfigValidationIssue[];

    constructor(issues: ConfigValidationIssue[]) {
        super(
            `Invalid oracle service configuration:\n${issues
                .map((i) => `  - ${i.path}: ${i.message}`)
                .join('\n')}`,
        );
        this.name = 'ConfigValidationError';
        this.issues = issues;
    }
}

class IssueCollector {
    readonly issues: ConfigValidationIssue[] = [];

    add(path: string, message: string): void {
        this.issues.push({ path, message });
    }

    get failed(): boolean {
        return this.issues.length > 0;
    }
}

/** True for a plain (non-null, non-array) object. */
function isPlainObject(value: unknown): value is Record<string, unknown> {
    return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/**
 * Validates a URL that the service will actually dial.
 * Rejects anything that is not an absolute http(s) URL with a host.
 */
function isUsableHttpUrl(value: unknown): value is string {
    if (typeof value !== 'string' || value.trim() !== value || value.length === 0) {
        return false;
    }
    let parsed: URL;
    try {
        parsed = new URL(value);
    } catch {
        return false;
    }
    return (parsed.protocol === 'http:' || parsed.protocol === 'https:') && parsed.host !== '';
}

/**
 * Validates a cache backend URL. Redis uses its own scheme, so `redis:` and
 * `rediss:` are accepted in addition to http(s) proxies.
 */
function isUsableRedisUrl(value: unknown): value is string {
    if (typeof value !== 'string' || value.trim() !== value || value.length === 0) {
        return false;
    }
    let parsed: URL;
    try {
        parsed = new URL(value);
    } catch {
        return false;
    }
    return (
        ['redis:', 'rediss:', 'http:', 'https:'].includes(parsed.protocol) &&
        parsed.host !== ''
    );
}

/** True for a finite number strictly greater than zero. */
function isPositiveFinite(value: unknown): value is number {
    return typeof value === 'number' && Number.isFinite(value) && value > 0;
}

/** True for a finite number greater than or equal to zero. */
function isNonNegativeFinite(value: unknown): value is number {
    return typeof value === 'number' && Number.isFinite(value) && value >= 0;
}

/** True for a safe integer. */
function isSafeInteger(value: unknown): value is number {
    return typeof value === 'number' && Number.isSafeInteger(value);
}

function validateProvider(
    provider: unknown,
    index: number,
    collector: IssueCollector,
    seenNames: Map<string, number>,
): void {
    const path = `providers[${index}]`;

    if (!isPlainObject(provider)) {
        collector.add(path, 'must be an object');
        return;
    }

    const name = provider.name;
    if (typeof name !== 'string' || name.trim().length === 0) {
        collector.add(`${path}.name`, 'must be a non-empty string');
    } else {
        const previous = seenNames.get(name);
        if (previous !== undefined) {
            collector.add(
                `${path}.name`,
                `duplicate provider name "${name}" (already declared at providers[${previous}])`,
            );
        } else {
            seenNames.set(name, index);
        }

        if (!(SUPPORTED_PROVIDER_NAMES as readonly string[]).includes(name)) {
            collector.add(
                `${path}.name`,
                `unknown provider "${name}"; supported providers are ${SUPPORTED_PROVIDER_NAMES.join(', ')}`,
            );
        }
    }

    if (typeof provider.enabled !== 'boolean') {
        collector.add(`${path}.enabled`, 'must be a boolean');
    }

    if (!isSafeInteger(provider.priority) || provider.priority < 1) {
        collector.add(
            `${path}.priority`,
            'must be an integer greater than or equal to 1',
        );
    }

    if (!Number.isFinite(provider.weight) || (provider.weight as number) < 0 || (provider.weight as number) > 1) {
        collector.add(`${path}.weight`, 'must be a finite number between 0 and 1 inclusive');
    }

    if (typeof provider.apiKey === 'string' && provider.apiKey.trim().length === 0) {
        collector.add(`${path}.apiKey`, 'must be omitted rather than an empty string');
    }

    if (!isUsableHttpUrl(provider.baseUrl)) {
        collector.add(`${path}.baseUrl`, 'must be an absolute http(s) URL');
    }

    const rateLimit = provider.rateLimit;
    if (!isPlainObject(rateLimit)) {
        collector.add(`${path}.rateLimit`, 'must be an object');
        return;
    }

    if (!isSafeInteger(rateLimit.maxRequests) || rateLimit.maxRequests < 1) {
        collector.add(`${path}.rateLimit.maxRequests`, 'must be an integer greater than or equal to 1');
    }
    if (!isSafeInteger(rateLimit.windowMs) || rateLimit.windowMs < 1) {
        collector.add(`${path}.rateLimit.windowMs`, 'must be an integer greater than or equal to 1');
    }
}

function validatePriceBounds(
    bounds: unknown,
    collector: IssueCollector,
): void {
    if (bounds === undefined) {
        return;
    }

    if (!isPlainObject(bounds)) {
        collector.add('priceBounds', 'must be an object keyed by asset symbol');
        return;
    }

    for (const [asset, value] of Object.entries(bounds)) {
        const path = `priceBounds.${asset}`;

        if (!isPlainObject(value)) {
            collector.add(path, 'must be an object with minPrice and maxPrice');
            continue;
        }
        if (!isPositiveFinite(value.minPrice)) {
            collector.add(`${path}.minPrice`, 'must be a finite number greater than 0');
        }
        if (!Number.isFinite(value.maxPrice) || (value.maxPrice as number) < (value.minPrice as number)) {
            collector.add(
                `${path}.maxPrice`,
                'must be a finite number greater than or equal to minPrice',
            );
        }
    }
}

/**
 * Validate a service configuration and return a normalized copy.
 *
 * Normalization is limited to provider weight scaling so that the effective
 * weights reported at runtime always describe a true distribution. All other
 * fields are returned untouched; this function never mutates its input and
 * never reaches for the environment, network, or clock, so its result is
 * deterministic for a given input.
 *
 * @throws {ConfigValidationError} when any rule is violated.
 */
export function validateOracleServiceConfig(config: unknown): OracleServiceConfig {
    const collector = new IssueCollector();

    if (!isPlainObject(config)) {
        throw new ConfigValidationError([
            { path: '', message: 'configuration must be an object' },
        ]);
    }

    if (!(NETWORKS as readonly unknown[]).includes(config.stellarNetwork)) {
        collector.add('stellarNetwork', `must be one of ${NETWORKS.join(' | ')}`);
    }

    if (!isUsableHttpUrl(config.stellarRpcUrl)) {
        collector.add('stellarRpcUrl', 'must be an absolute http(s) URL');
    }

    if (
        typeof config.contractId !== 'string' ||
        !/^[A-Z0-9]{4,64}$/.test(config.contractId)
    ) {
        collector.add('contractId', 'must be 4-64 uppercase alphanumeric characters');
    }

    if (typeof config.adminSecretKey !== 'string' || config.adminSecretKey.trim().length === 0) {
        collector.add('adminSecretKey', 'must be a non-empty string');
    }

    if (!isSafeInteger(config.updateIntervalMs) || config.updateIntervalMs < 1) {
        collector.add('updateIntervalMs', 'must be an integer greater than or equal to 1');
    }

    if (!isPositiveFinite(config.maxPriceDeviationPercent) || config.maxPriceDeviationPercent > 100) {
        collector.add(
            'maxPriceDeviationPercent',
            'must be a finite number greater than 0 and at most 100',
        );
    }

    if (config.madZScoreThreshold !== undefined && !isNonNegativeFinite(config.madZScoreThreshold)) {
        collector.add(
            'madZScoreThreshold',
            'must be a finite number greater than or equal to 0 when provided',
        );
    }

    if (!isPositiveFinite(config.priceStaleThresholdSeconds)) {
        collector.add('priceStaleThresholdSeconds', 'must be a finite number greater than 0');
    }

    // 0 is a supported value: it disables caching without failing startup.
    if (!isNonNegativeFinite(config.cacheTtlSeconds)) {
        collector.add('cacheTtlSeconds', 'must be a finite number greater than or equal to 0');
    }

    if (!(LOG_LEVELS as readonly unknown[]).includes(config.logLevel)) {
        collector.add('logLevel', `must be one of ${LOG_LEVELS.join(' | ')}`);
    }

    if (config.redisUrl !== undefined && config.redisUrl !== '' && !isUsableRedisUrl(config.redisUrl)) {
        collector.add(
            'redisUrl',
            'must be omitted, empty, or a redis://, rediss://, http:// or https:// URL',
        );
    }

    // The admin API is privileged: it may only be enabled with a strong secret.
    const adminApiPort = config.adminApiPort;
    if (adminApiPort !== undefined) {
        if (!isSafeInteger(adminApiPort) || adminApiPort < 0 || adminApiPort > 65535) {
            collector.add('adminApiPort', 'must be an integer between 0 and 65535');
        } else if (adminApiPort > 0) {
            const secret = config.adminHmacSecret;
            if (typeof secret !== 'string' || secret.length < 16) {
                collector.add(
                    'adminHmacSecret',
                    'is required and must be at least 16 characters when adminApiPort is greater than 0',
                );
            }
        }
    }

    validatePriceBounds(config.priceBounds, collector);

    // Providers
    const providers = config.providers;

    if (!Array.isArray(providers)) {
        collector.add('providers', 'must be an array');
    } else if (providers.length === 0) {
        collector.add('providers', 'must declare at least one provider');
    } else {
        const issueCountBefore = collector.issues.length;
        const seenNames = new Map<string, number>();
        providers.forEach((provider, index) =>
            validateProvider(provider, index, collector, seenNames),
        );
        const providersWellFormed = collector.issues.length === issueCountBefore;

        // Only reason about the weight distribution when every entry is
        // well-formed; otherwise the reported problems would be noise.
        if (providersWellFormed) {
            const enabled = providers.filter(
                (p): p is ProviderConfig => isPlainObject(p) && p.enabled === true,
            );

            if (enabled.length === 0) {
                collector.add('providers', 'at least one provider must be enabled');
            } else {
                const total = enabled.reduce((sum, p) => sum + p.weight, 0);
                if (total <= 0) {
                    collector.add(
                        'providers',
                        'the sum of enabled provider weights must be greater than 0',
                    );
                } else if (total > 1) {
                    collector.add(
                        'providers',
                        `the sum of enabled provider weights must not exceed 1 (got ${total})`,
                    );
                }
            }
        }
    }

    if (collector.failed) {
        throw new ConfigValidationError(collector.issues);
    }

    return normalizeConfig(config as unknown as OracleServiceConfig);
}

/**
 * Scale enabled provider weights so they sum to exactly 1.
 * Disabled providers keep their configured weight but take no part in scaling.
 */
function normalizeConfig(config: OracleServiceConfig): OracleServiceConfig {
    const providers = config.providers ?? [];
    const total = providers
        .filter((p) => p.enabled)
        .reduce((sum, p) => sum + p.weight, 0);

    if (!Number.isFinite(total) || total <= 0) {
        return { ...config, providers: providers.map((p) => ({ ...p })) };
    }

    return {
        ...config,
        providers: providers.map((p) => ({
            ...p,
            rateLimit: { ...p.rateLimit },
            weight: p.enabled ? p.weight / total : p.weight,
        })),
    };
}

