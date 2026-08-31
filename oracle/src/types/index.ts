/**
 * Oracle Service Type Definitions
 *
 * This module contains all TypeScript interfaces and types used across
 * the Oracle Integration Service for StellarLend protocol.
 */

// ─── Core price types ─────────────────────────────────────────────────────────

export interface PriceData {
    asset: string;
    price: bigint;
    timestamp: number;
    source: string;
    confidence: number;
    /** 24-hour quote volume in USD. Used as weight in aggregation. */
    volume24h?: bigint;
    /** Optional signer public key when providers sign payloads */
    signer?: string;
    /** Optional signature over the canonical payload (hex/base64) */
    signature?: string;
}

export interface RawPriceData {
    asset: string;
    price: number;
    timestamp: number;
    source: string;
    volume24h?: bigint;
    signer?: string;
    signature?: string;
}

export interface AggregatedPrice {
    asset: string;
    price: bigint;
    sources: PriceData[];
    timestamp: number;
    confidence: number;
    /** Monotonic sequence number to enforce ordering and prevent stale updates. */
    sequence?: number;
}

// ─── Validation ───────────────────────────────────────────────────────────────

export interface ValidationResult {
    isValid: boolean;
    price?: PriceData;
    errors: ValidationError[];
}

export interface ValidationError {
    code: ValidationErrorCode;
    message: string;
    details?: Record<string, unknown>;
}

export enum ValidationErrorCode {
    PRICE_ZERO = 'PRICE_ZERO',
    PRICE_NEGATIVE = 'PRICE_NEGATIVE',
    PRICE_STALE = 'PRICE_STALE',
    PRICE_DEVIATION_TOO_HIGH = 'PRICE_DEVIATION_TOO_HIGH',
    PRICE_BELOW_MIN = 'PRICE_BELOW_MIN',
    PRICE_ABOVE_MAX = 'PRICE_ABOVE_MAX',
    INVALID_ASSET = 'INVALID_ASSET',
    SOURCE_UNAVAILABLE = 'SOURCE_UNAVAILABLE',
    DUPLICATE_SUBMISSION = 'DUPLICATE_SUBMISSION',
    INVALID_STATE_TRANSITION = 'INVALID_STATE_TRANSITION',
    RECOVERY_IN_PROGRESS = 'RECOVERY_IN_PROGRESS',
}

// ─── Provider / cache ─────────────────────────────────────────────────────────

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

export interface CacheEntry<T> {
    data: T;
    cachedAt: number;
    expiresAt: number;
}

// ─── Contract update ──────────────────────────────────────────────────────────

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

// ─── Service configuration ────────────────────────────────────────────────────

export interface AssetPriceBounds {
    minPrice: number;
    maxPrice: number;
}

export interface OracleServiceConfig {
    stellarNetwork: 'testnet' | 'mainnet';
    stellarRpcUrl: string;
    contractId: string;
    adminSecretKey: string;
    /** Port for the admin HTTP API (0 or omitted disables the server). */
    adminApiPort?: number;
    /** HMAC-SHA256 secret. Required when adminApiPort > 0. */
    adminHmacSecret?: string;
    updateIntervalMs: number;
    maxPriceDeviationPercent: number;
    /** MAD z-score threshold for outlier rejection (default 3.5). */
    madZScoreThreshold?: number;
    priceStaleThresholdSeconds: number;
    cacheTtlSeconds: number;
    redisUrl?: string;
    logLevel: 'debug' | 'info' | 'warn' | 'error';
    providers: ProviderConfig[];
    /** Per-asset price bounds enforced by the validator. */
    priceBounds?: Partial<Record<SupportedAsset, AssetPriceBounds>>;
    /** Freshness policy governing stale data handling. */
    freshnessPolicy?: FreshnessPolicy;
    /** Fallback policy governing provider fallback and aggregation. */
    fallbackPolicy?: FallbackPolicy;
    /** Recovery policy for interrupted operations. */
    recoveryPolicy?: RecoveryPolicy;
}

// ─── Telemetry / diagnostics ──────────────────────────────────────────────────

/**
 * Outcome of a single provider fetch attempt within one update cycle.
 * Error classes are normalised — raw messages are never included.
 */
export interface ProviderFetchEvent {
    provider: string;
    asset: string;
    /** Wall-clock latency in milliseconds. */
    latencyMs: number;
    success: boolean;
    errorClass?: 'network' | 'rate_limit' | 'validation' | 'timeout' | 'unknown';
}

/**
 * Diagnostics emitted at the end of each price-update cycle.
 * Exposes latency, failure, and recovery signals without leaking secrets.
 */
export interface UpdateCycleDiagnostics {
    startedAt: string;
    durationMs: number;
    assetsUpdated: number;
    assetsFailed: number;
    providerEvents: ProviderFetchEvent[];
    cooledDownProviders: string[];
    contractUpdateOk: boolean;
}

// ─── Supported assets ─────────────────────────────────────────────────────────

export type SupportedAsset = 'XLM' | 'USDC' | 'USDT' | 'BTC' | 'ETH';

export interface AssetMapping {
    symbol: SupportedAsset;
    coingeckoId: string;
    coinmarketcapId: number;
    binanceSymbol: string;
}

// ─── Health / metrics ─────────────────────────────────────────────────────────

export interface HealthStatus {
    provider: string;
    healthy: boolean;
    lastCheck: number;
    latencyMs?: number;
    error?: string;
}

export interface ServiceMetrics {
    priceUpdatesTotal: number;
    priceUpdatesFailed: number;
    cacheHits: number;
    cacheMisses: number;
    providerErrors: Map<string, number>;
    lastUpdateTimestamp: number;
}

// ─── State machine (upstream: idempotency / session tracking) ─────────────────

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
    RECOVERING = 'RECOVERING',
}

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

// ─── Policy types (upstream: freshness / fallback / recovery) ─────────────────

export interface FreshnessPolicy {
    maxStalenessSeconds: number;
    maxDeviationPercent: number;
    requireFresh: boolean;
    fallbackOnStale: boolean;
}

export interface FallbackPolicy {
    enabled: boolean;
    fallbackOrder: 'priority' | 'round-robin';
    preferHighestConfidence: boolean;
    minSources: number;
    useVolumeWeightedMedian: boolean;
    maxFallbackAttempts: number;
}

export interface RecoveryPolicy {
    enabled: boolean;
    preserveUserIntent: boolean;
    idempotentRetries: boolean;
    resumeFromPersistedState: boolean;
    statePersistence: 'none' | 'memory' | 'redis';
    timeoutSeconds: number;
}

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
