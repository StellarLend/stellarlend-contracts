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
    /** 24-hour quote volume in USD, carried from the raw provider response. Used as weight in aggregation. */
    volume24h?: bigint;
}

/**
 * Raw price data before validation and conversion
 */
export interface RawPriceData {
    asset: string;
    price: number;
    timestamp: number;
    source: string;
    /** 24-hour quote volume in USD (integer, scaled to avoid floats). Used as weight in aggregation. */
    volume24h?: bigint;
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
    /** Monotonic sequence number to enforce ordering and prevent stale updates. */
    sequence?: number;
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
    PRICE_BELOW_MIN = 'PRICE_BELOW_MIN',
    PRICE_ABOVE_MAX = 'PRICE_ABOVE_MAX',
    INVALID_ASSET = 'INVALID_ASSET',
    SOURCE_UNAVAILABL = 'SOURCE_UNAVAILABL',
    DUPLICATE_SUBMISSION = 'DUPLICATE_SUBMISSION',
    INVALID_STATE_TRANSITION = 'INVALID_STATE_TRANSITION',
    RECOVERY_IN_PROGRESS = 'RECOVERY_IN_PROGRESS',
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
    /** Idempotency key to prevent duplicate on-chain actions. */
    idempotencyKey?: string;
    /** Sequence number to ensure stale responses are not applied. */
    sequence?: number;
    /** Final session state after the update attempt. */
    sessionState?: PriceUpdateState;
}

/**
 * Service configuration
 */
export interface AssetPriceBounds {
    minPrice: number;
    maxPrice: number;
}

export interface OracleServiceConfig {
    stellarNetwork: 'testnet' | 'mainnet';
    stellarRpcUrl: string;
    contractId: string;
    adminSecretKey: string;
    adminApiPort: number;
    adminHmacSecret?: string;
    updateIntervalMs: number;
    maxPriceDeviationPercent: number;
    madZ?ScoreThreshold: number;
    priceStaleThresholdSeconds: number;
    cacheTtlSeconds: number;
    redisUrl?: string;
    logLevel: 'debug' | 'info' | 'warn' | 'error';
    providers: ProviderConfig[];
    priceBounds: Record<SupportedAsset, AssetPriceBounds>;
    /** Freshness policy governing stale data handling. */
    freshnessPolicy?: FreshnessPolicy;
    /** Fallback policy governing provider fallback and aggregation. */
    fallbackPolicy?: FallbackPolicy;
    /** Recovery policy for interrupted operations. */
    recoveryPolicy?: RecoveryPolicy;
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

/**
 * Price update state machine states.
 * Each state maps to a distinct phase in the oracle update transaction lifecycle.
 */
export enum PriceUpdateState {
    IDLE = 'IDLE',
    FETCHING = 'FETCHING',
    VALIDATING = 'VALIDATING',
    AGGREGATING = 'AGGREGATING',
    SUBMITTING = 'SUBMITTING',
    SUCCESS = 'SUCCESS',
    FAILED = 'FAILED',
    RETRYING = 'RETRYING',
    CANCELLED = 'CANCELLED',
    RECOVERING = 'RECOVERING,
}

/**
 * Defined transitions for the price update state machine.
 * This fully specifies valid state transitions and ensures deterministic behavior.
 */
export const PriceUpdateStateTransitions: Record<PriceUpdateState, PriceUpdateState[]> = {
    [PriceUpdateState.IDLE]: [PriceUpdateState.FETCHING, PriceUpdateState.CANCELLED],
    [PriceUpdateState.FETCHING]: [PriceUpdateState.VALIDATING, PriceUpdateState.FAILED, PriceUpdateState.CANCELLED],
    [PriceUpdateState.VALIDATING]: [PriceUpdateState.AGGREGATING, PriceUpdateState.FAILED, PriceUpdateState.CANCELLED],
    [PriceUpdateState.AGGREGATING]: [PriceUpdateState.SUBMITTING, PriceUpdateState.FAILED, PriceUpdateState.CANCELLED],
    [PriceUpdateState.SUBMITTING]: [PriceUpdateState.SUCCESS, PriceUpdateState.FAILED, PriceUpdateState.RETRYING, PriceUpdateState.CANCELLED],
    [PriceUpdateState.SUCCESS]: [],
    [PriceUpdateState.FAILED]: [PriceUpdateState.RETRYING, PriceUpdateState.RECOVERING, PriceUpdateState.CANCELLED],
    [PriceUpdateState.RETRYING]: [PriceUpdateState.FETCHING, PriceUpdateState.SUBMITTING, PriceUpdateState.FAILED, PriceUpdateState.CANCELLED],
    [PriceUpdateState.CANCELLED]: [],
    [PriceUpdateState.RECOVERING]: [PriceUpdateState.FETCHING, PriceUpdateState.SUBMITTING, PriceUpdateState.FAILED, PriceUpdateState.CANCELLED],
};

/**
 * Context for a single price update session.
 * Tracks state, attempt count, idempotency, and recovery metadata.
 */
export interface PriceUpdateSession {
    sessionId: string;
    asset: SupportedAsset;
    state: PriceUpdateState;
    attemptCount: number;
    maxAttempts: number;
    createdAt: number;
    updatedAt: number;
    lastError?: ValidationError;
    idempotencyKey?: string;
    transactionHash?: string;
    requestedAt: number;
    recoveryState?: Record<string, unknown>;
    userIntent?: string;
}

/**
 * Policy governing freshness enforcement and stale-data fallback.
 */
export interface FreshnessPolicy {
    maxStalenessSeconds: number;
    maxDeviationPercent: number;
    requireFresh: boolean;
    fallbackOnStale: boolean;
}

/**
 * Policy governing provider fallback and data aggregation.
 */
export interface FallbackPolicy {
    enabled: boolean;
    fallbackOrder: 'priority' | 'round-robin';
    preferHighestConfidence: boolean;
    minSources: number;
    useVolumeWeightedMedian: boolean;
    maxFallbackAttempts: number;
}

/**
 * Policy governing recovery after interruptions or failed on-chain submissions.
 */
export interface RecoveryPolicy {
    enabled: boolean;
    preserveUserIntent: boolean;
    idempotentRetries: boolean;
    resumeFromPersistedState: boolean;
    statePersistence: 'none' | 'memory' | 'redis';
    timeoutSeconds: number;
}

/**
 * A serializable receipt that proves an on-chain submission was attempted.
 * Used to prevent duplicate submissions and enable recovery.
 */
export interface SubmissionReceipt {
    idempotencyKey: string;
    asset: SupportedAsset;
    price: bigint;
    timestamp: number;
    transactionHash?: string;
    success: boolean;
    attempt: number;
    submittedAt: number;
}
