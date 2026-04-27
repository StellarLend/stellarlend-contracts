/**
 * Oracle Service Type Definitions
 * 
 * This module contains all TypeScript interfaces and types used across
 * the Oracle Integration Service for StellarLend protocol.
 */

/**
 * Represents price data fetched from an external source
 */
export interface PriceData {
    asset: string;
    price: bigint;
    timestamp: number;
    source: string;
    confidence: number;
}

/**
 * Raw price data before validation and conversion
 */
export interface RawPriceData {
    asset: string;
    price: number;
    timestamp: number;
    source: string;
}

/**
 * Aggregated price from multiple sources
*/
export interface AggregatedPrice {
    asset: string;
    price: bigint;
    sources: PriceData[];
    timestamp: number;
    confidence: number;
}

/**
 * Price validation result
 */
export interface ValidationResult {
    isValid: boolean;
    price?: PriceData;
    errors: ValidationError[];
}

/**
 * Validation error details
 */
export interface ValidationError {
    code: ValidationErrorCode;
    message: string;
    details?: Record<string, unknown>;
}

/**
 * Validation error codes
 */
export enum ValidationErrorCode {
    PRICE_ZERO = 'PRICE_ZERO',
    PRICE_NEGATIVE = 'PRICE_NEGATIVE',
    PRICE_STALE = 'PRICE_STALE',
    PRICE_DEVIATION_TOO_HIGH = 'PRICE_DEVIATION_TOO_HIGH',
    INVALID_ASSET = 'INVALID_ASSET',
    SOURCE_UNAVAILABLE = 'SOURCE_UNAVAILABLE',
}

/**
 * Provider configuration
 */
export interface ProviderConfig {
    name: string;
    enabled: boolean;
    priority: number;
    weight: number;
    apiKey?: string;
    baseUrl: string;
    rateLimit: {
        maxRequests: number;
        windowMs: number;
    };
}

/**
 * Cache entry structure
 */
export interface CacheEntry<T> {
    data: T;
    cachedAt: number;
    expiresAt: number;
}

/**
 * Contract update result
 */
export interface ContractUpdateResult {
    success: boolean;
    transactionHash?: string;
    asset: string;
    price: bigint;
    timestamp: number;
    error?: string;
}

/**
 * Service configuration
 */
export interface OracleServiceConfig {
    stellarNetwork: 'testnet' | 'mainnet';
    stellarRpcUrl: string;
    contractId: string;
    adminSecretKey: string;
    updateIntervalMs: number;
    maxPriceDeviationPercent: number;
    priceStaleThresholdSeconds: number;
    cacheTtlSeconds: number;
    redisUrl?: string;
    logLevel: 'debug' | 'info' | 'warn' | 'error';
    providers: ProviderConfig[];
}

/**
 * Supported assets for price fetching
 */
export type SupportedAsset =
    | 'XLM'
    | 'USDC'
    | 'USDT'
    | 'BTC'
    | 'ETH';

/**
 * Asset mapping for different providers
 */
export interface AssetMapping {
    symbol: SupportedAsset;
    coingeckoId: string;
    coinmarketcapId: number;
    binanceSymbol: string;
}

/**
 * Health check status
 */
export interface HealthStatus {
    provider: string;
    healthy: boolean;
    lastCheck: number;
    latencyMs?: number;
    error?: string;
}

/**
 * Service metrics for monitoring
 */
export interface ServiceMetrics {
    priceUpdatesTotal: number;
    priceUpdatesFailed: number;
    cacheHits: number;
    cacheMisses: number;
    providerErrors: Map<string, number>;
    lastUpdateTimestamp: number;
}
