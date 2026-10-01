/**
 * Oracle Service Type Definitions
 *
 * This module contains all TypeScript interfaces, types, constants, state machine
 * transitions, and boundary validation utilities used across the Oracle Integration
 * Service for StellarLend protocol.
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
  /** Optional signer public key when providers sign payloads */
  signer?: string;
  /** Optional signature over the canonical payload (hex/base64) */
  signature?: string;
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
  /** Optional signer public key when providers sign payloads */
  signer?: string;
  /** Optional signature over the canonical payload (hex/base64) */
  signature?: string;
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
  PRICE_ZERO = "PRICE_ZERO",
  PRICE_NEGATIVE = "PRICE_NEGATIVE",
  PRICE_STALE = "PRICE_STALE",
  PRICE_DEVIATION_TOO_HIGH = "PRICE_DEVIATION_TOO_HIGH",
  PRICE_BELOW_MIN = "PRICE_BELOW_MIN",
  PRICE_ABOVE_MAX = "PRICE_ABOVE_MAX",
  INVALID_ASSET = "INVALID_ASSET",
  SOURCE_UNAVAILABLE = "SOURCE_UNAVAILABLE",
  SOURCE_UNAVAILABL = "SOURCE_UNAVAILABL", // Backwards-compatible alias for previous typo
  DUPLICATE_SUBMISSION = "DUPLICATE_SUBMISSION",
  INVALID_STATE_TRANSITION = "INVALID_STATE_TRANSITION",
  RECOVERY_IN_PROGRESS = "RECOVERY_IN_PROGRESS",
  UNAUTHORIZED = "UNAUTHORIZED",
  INVALID_INPUT = "INVALID_INPUT",
  MAX_RETRIES_EXCEEDED = "MAX_RETRIES_EXCEEDED",
  SEQUENCE_REGRESSION = "SEQUENCE_REGRESSION",
  PARTIAL_FAILURE = "PARTIAL_FAILURE",
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
    adminApiPort?: number;
    adminHmacSecret?: string;
    updateIntervalMs: number;
    maxPriceDeviationPercent: number;
    madZScoreThreshold: number;
    priceStaleThresholdSeconds: number;
    cacheTtlSeconds: number;
    redisUrl?: string;
    logLevel: 'debug' | 'info' | 'warn' | 'error';
    providers: ProviderConfig[];
    priceBounds?: Record<SupportedAsset, AssetPriceBounds>;
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
export type SupportedAsset = "XLM" | "USDC" | "USDT" | "BTC" | "ETH";

export const SUPPORTED_ASSETS: readonly SupportedAsset[] = [
  "XLM",
  "USDC",
  "USDT",
  "BTC",
  "ETH",
] as const;

/**
 * Check if an asset symbol is supported
 */
export function isSupportedAsset(asset: unknown): asset is SupportedAsset {
  return (
    typeof asset === "string" &&
    SUPPORTED_ASSETS.includes(asset as SupportedAsset)
  );
}

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
    RECOVERING = 'RECOVERING',
}

/**
 * Defined transitions for the price update state machine.
 * Fully specifies valid state transitions and ensures deterministic behavior.
 */
export const PriceUpdateStateTransitions: Record<
  PriceUpdateState,
  readonly PriceUpdateState[]
> = {
  [PriceUpdateState.IDLE]: [
    PriceUpdateState.FETCHING,
    PriceUpdateState.CANCELLED,
  ],
  [PriceUpdateState.FETCHING]: [
    PriceUpdateState.VALIDATING,
    PriceUpdateState.FAILED,
    PriceUpdateState.CANCELLED,
  ],
  [PriceUpdateState.VALIDATING]: [
    PriceUpdateState.AGGREGATING,
    PriceUpdateState.FAILED,
    PriceUpdateState.CANCELLED,
  ],
  [PriceUpdateState.AGGREGATING]: [
    PriceUpdateState.SUBMITTING,
    PriceUpdateState.FAILED,
    PriceUpdateState.CANCELLED,
  ],
  [PriceUpdateState.SUBMITTING]: [
    PriceUpdateState.SUCCESS,
    PriceUpdateState.FAILED,
    PriceUpdateState.RETRYING,
    PriceUpdateState.CANCELLED,
  ],
  [PriceUpdateState.SUCCESS]: [],
  [PriceUpdateState.FAILED]: [
    PriceUpdateState.RETRYING,
    PriceUpdateState.RECOVERING,
    PriceUpdateState.CANCELLED,
  ],
  [PriceUpdateState.RETRYING]: [
    PriceUpdateState.FETCHING,
    PriceUpdateState.SUBMITTING,
    PriceUpdateState.FAILED,
    PriceUpdateState.CANCELLED,
  ],
  [PriceUpdateState.CANCELLED]: [],
  [PriceUpdateState.RECOVERING]: [
    PriceUpdateState.FETCHING,
    PriceUpdateState.SUBMITTING,
    PriceUpdateState.FAILED,
    PriceUpdateState.CANCELLED,
  ],
};

/**
 * Validates whether a state transition is permitted by the state machine.
 */
export function isValidStateTransition(from: unknown, to: unknown): boolean {
  if (typeof from !== "string" || typeof to !== "string") {
    return false;
  }
  const allowed = PriceUpdateStateTransitions[from as PriceUpdateState];
  if (!allowed) {
    return false;
  }
  return allowed.includes(to as PriceUpdateState);
}

/**
 * Asserts valid state transition, throwing a structured ValidationError if invalid.
 */
export function assertValidStateTransition(
  from: PriceUpdateState,
  to: PriceUpdateState,
): void {
  if (!isValidStateTransition(from, to)) {
    throw createValidationError(
      ValidationErrorCode.INVALID_STATE_TRANSITION,
      `Invalid state transition from '${from}' to '${to}'`,
      { fromState: from, toState: to },
    );
  }
}

/**
 * Identifies if a state is terminal (no outgoing transitions possible).
 */
export function isTerminalState(state: PriceUpdateState): boolean {
  const transitions = PriceUpdateStateTransitions[state];
  return !transitions || transitions.length === 0;
}

/**
 * Checks whether a state is eligible for retry attempts.
 */
export function isRetryableState(state: PriceUpdateState): boolean {
  return (
    state === PriceUpdateState.FAILED ||
    state === PriceUpdateState.RETRYING ||
    state === PriceUpdateState.RECOVERING
  );
}

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
  fallbackOrder: "priority" | "round-robin";
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
  statePersistence: "none" | "memory" | "redis";
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

// ---------------------------------------------------------------------------
// Security & Sanitization Helpers (Diagnosability without leaking secrets)
// ---------------------------------------------------------------------------

const SENSITIVE_KEY_REGEX =
  /key|secret|token|password|auth|credential|admin|private/i;
const STELLAR_SECRET_REGEX = /S[A-Z2-7]{55}/g;

/**
 * Sanitizes details to ensure sensitive keys and tokens are redacted.
 */
export function sanitizeSensitiveData(val: unknown, depth = 0): unknown {
  if (depth > 5 || val === null || val === undefined) return val;
  if (typeof val === "string") {
    return val.replace(STELLAR_SECRET_REGEX, "S***[REDACTED]");
  }
  if (typeof val === "bigint") {
    return val.toString();
  }
  if (Array.isArray(val)) {
    return val.map((item) => sanitizeSensitiveData(item, depth + 1));
  }
  if (typeof val === "object") {
    const sanitized: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(val as Record<string, unknown>)) {
      if (SENSITIVE_KEY_REGEX.test(k)) {
        sanitized[k] = "[REDACTED]";
      } else {
        sanitized[k] = sanitizeSensitiveData(v, depth + 1);
      }
    }
    return sanitized;
  }
  return val;
}

/**
 * Construct a ValidationError with sanitized message and details to ensure safe observability.
 */
export function createValidationError(
  code: ValidationErrorCode,
  message: string,
  details?: Record<string, unknown>,
): ValidationError {
  const cleanMessage =
    typeof message === "string"
      ? message.replace(STELLAR_SECRET_REGEX, "S***[REDACTED]")
      : "Validation error";
  const cleanDetails = details
    ? (sanitizeSensitiveData(details) as Record<string, unknown>)
    : undefined;

  return {
    code,
    message: cleanMessage,
    details: cleanDetails,
  };
}

// ---------------------------------------------------------------------------
// Price Update Session Management & Retry / Concurrency Boundaries
// ---------------------------------------------------------------------------

export interface CreateSessionOptions {
  sessionId: string;
  asset: SupportedAsset;
  maxAttempts?: number;
  idempotencyKey?: string;
  userIntent?: string;
  requestedAt?: number;
}

/**
 * Creates an immutable price update session with boundary validation.
 */
export function createPriceUpdateSession(
  options: CreateSessionOptions,
): PriceUpdateSession {
  if (!options || typeof options !== "object") {
    throw createValidationError(
      ValidationErrorCode.INVALID_INPUT,
      "Session options are required",
    );
  }
  if (
    !options.sessionId ||
    typeof options.sessionId !== "string" ||
    options.sessionId.trim().length === 0
  ) {
    throw createValidationError(
      ValidationErrorCode.INVALID_INPUT,
      "sessionId must be a non-empty string",
    );
  }
  if (!isSupportedAsset(options.asset)) {
    throw createValidationError(
      ValidationErrorCode.INVALID_ASSET,
      `Unsupported asset: ${options.asset}`,
      { asset: options.asset },
    );
  }
  const maxAttempts = options.maxAttempts ?? 3;
  if (
    typeof maxAttempts !== "number" ||
    !Number.isInteger(maxAttempts) ||
    maxAttempts <= 0
  ) {
    throw createValidationError(
      ValidationErrorCode.INVALID_INPUT,
      "maxAttempts must be a positive integer >= 1",
      { maxAttempts },
    );
  }

  const now = Date.now();
  const requestedAt =
    typeof options.requestedAt === "number" && options.requestedAt > 0
      ? options.requestedAt
      : now;

  const idempotencyKey =
    options.idempotencyKey ??
    generateIdempotencyKey(options.asset, requestedAt);

  return Object.freeze({
    sessionId: options.sessionId.trim(),
    asset: options.asset,
    state: PriceUpdateState.IDLE,
    attemptCount: 0,
    maxAttempts,
    createdAt: now,
    updatedAt: now,
    requestedAt,
    idempotencyKey,
    userIntent: options.userIntent,
  });
}

/**
 * Transitions a session to the next state, strictly checking transitions and maxAttempts.
 * Protects against race conditions by returning a new frozen session instance.
 */
export function transitionSession(
  session: PriceUpdateSession,
  nextState: PriceUpdateState,
  options?: {
    error?: ValidationError;
    transactionHash?: string;
    recoveryState?: Record<string, unknown>;
  },
): PriceUpdateSession {
  if (!session || typeof session !== "object") {
    throw createValidationError(
      ValidationErrorCode.INVALID_INPUT,
      "Invalid session object",
    );
  }

  assertValidStateTransition(session.state, nextState);

  let nextAttemptCount = session.attemptCount;
  if (nextState === PriceUpdateState.RETRYING) {
    if (session.attemptCount >= session.maxAttempts) {
      throw createValidationError(
        ValidationErrorCode.MAX_RETRIES_EXCEEDED,
        `Maximum retry attempts (${session.maxAttempts}) reached for session ${session.sessionId}`,
        {
          sessionId: session.sessionId,
          attemptCount: session.attemptCount,
          maxAttempts: session.maxAttempts,
        },
      );
    }
    nextAttemptCount += 1;
  }

  const now = Date.now();
  return Object.freeze({
    ...session,
    state: nextState,
    attemptCount: nextAttemptCount,
    updatedAt: now,
    lastError: options?.error ?? session.lastError,
    transactionHash: options?.transactionHash ?? session.transactionHash,
    recoveryState: options?.recoveryState ?? session.recoveryState,
  });
}

/**
 * Determines whether a session is safe and eligible to be retried.
 */
export function canRetrySession(session: PriceUpdateSession): boolean {
  if (!session || typeof session !== "object") return false;
  return (
    isRetryableState(session.state) &&
    session.attemptCount < session.maxAttempts
  );
}

/**
 * Validates session structural and operational invariants.
 */
export function validateSessionInvariants(session: unknown): ValidationResult {
  const errors: ValidationError[] = [];
  if (!session || typeof session !== "object") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Session must be a non-null object",
      ),
    );
    return { isValid: false, errors };
  }
  const s = session as Partial<PriceUpdateSession>;

  if (!s.sessionId || typeof s.sessionId !== "string") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "sessionId is required",
      ),
    );
  }
  if (!isSupportedAsset(s.asset)) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_ASSET,
        `Invalid asset ${s.asset}`,
      ),
    );
  }
  if (!s.state || !(s.state in PriceUpdateState)) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_STATE_TRANSITION,
        `Invalid state: ${s.state}`,
      ),
    );
  }
  if (typeof s.maxAttempts !== "number" || s.maxAttempts < 1) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "maxAttempts must be >= 1",
      ),
    );
  }
  if (typeof s.attemptCount !== "number" || s.attemptCount < 0) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "attemptCount cannot be negative",
      ),
    );
  }
  if (
    typeof s.maxAttempts === "number" &&
    typeof s.attemptCount === "number" &&
    s.attemptCount > s.maxAttempts
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.MAX_RETRIES_EXCEEDED,
        "attemptCount cannot exceed maxAttempts",
      ),
    );
  }
  if (
    typeof s.createdAt === "number" &&
    typeof s.updatedAt === "number" &&
    s.updatedAt < s.createdAt
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "updatedAt cannot precede createdAt",
      ),
    );
  }

  return {
    isValid: errors.length === 0,
    errors,
  };
}

// ---------------------------------------------------------------------------
// Idempotency, Sequence, and Submission Helpers
// ---------------------------------------------------------------------------

/**
 * Generates a deterministic idempotency key for an asset update.
 */
export function generateIdempotencyKey(
  asset: SupportedAsset,
  timestamp: number,
  sequence = 0,
): string {
  if (!isSupportedAsset(asset)) {
    throw createValidationError(
      ValidationErrorCode.INVALID_ASSET,
      `Invalid asset: ${asset}`,
    );
  }
  if (
    typeof timestamp !== "number" ||
    !Number.isFinite(timestamp) ||
    timestamp <= 0
  ) {
    throw createValidationError(
      ValidationErrorCode.INVALID_INPUT,
      "Timestamp must be a positive finite number",
    );
  }
  return `${asset.toUpperCase()}:${Math.floor(timestamp)}:${Math.max(0, Math.floor(sequence))}`;
}

/**
 * Validates the boundary and format of an idempotency key.
 */
export function validateIdempotencyKey(key: unknown): boolean {
  if (typeof key !== "string") return false;
  const trimmed = key.trim();
  return trimmed.length > 0 && trimmed.length <= 256;
}

/**
 * Creates an immutable submission receipt.
 */
export function createSubmissionReceipt(params: {
  idempotencyKey: string;
  asset: SupportedAsset;
  price: bigint;
  timestamp: number;
  transactionHash?: string;
  success: boolean;
  attempt: number;
}): SubmissionReceipt {
  if (!validateIdempotencyKey(params.idempotencyKey)) {
    throw createValidationError(
      ValidationErrorCode.INVALID_INPUT,
      "Invalid idempotencyKey",
    );
  }
  if (!isSupportedAsset(params.asset)) {
    throw createValidationError(
      ValidationErrorCode.INVALID_ASSET,
      `Invalid asset: ${params.asset}`,
    );
  }
  if (typeof params.price !== "bigint" || params.price <= 0n) {
    throw createValidationError(
      ValidationErrorCode.PRICE_ZERO,
      "Price must be positive bigint",
    );
  }
  if (typeof params.timestamp !== "number" || params.timestamp <= 0) {
    throw createValidationError(
      ValidationErrorCode.INVALID_INPUT,
      "Timestamp must be positive",
    );
  }
  if (typeof params.attempt !== "number" || params.attempt < 1) {
    throw createValidationError(
      ValidationErrorCode.INVALID_INPUT,
      "Attempt must be >= 1",
    );
  }

  return Object.freeze({
    idempotencyKey: params.idempotencyKey,
    asset: params.asset,
    price: params.price,
    timestamp: params.timestamp,
    transactionHash: params.transactionHash,
    success: Boolean(params.success),
    attempt: params.attempt,
    submittedAt: Date.now(),
  });
}

/**
 * Checks if a candidate submission is a duplicate of an existing submission.
 */
export function isDuplicateSubmission(
  existing: SubmissionReceipt,
  candidate: {
    idempotencyKey?: string;
    asset: SupportedAsset;
    timestamp: number;
  },
): boolean {
  if (!existing || !candidate) return false;
  if (
    candidate.idempotencyKey &&
    existing.idempotencyKey === candidate.idempotencyKey
  ) {
    return true;
  }
  return (
    existing.asset === candidate.asset &&
    existing.timestamp === candidate.timestamp
  );
}

/**
 * Validates sequence monotonic ordering between updates.
 */
export function validateSequenceOrder(
  currentSequence: number | undefined,
  nextSequence: number | undefined,
): ValidationResult {
  const errors: ValidationError[] = [];

  if (nextSequence !== undefined) {
    if (
      typeof nextSequence !== "number" ||
      !Number.isInteger(nextSequence) ||
      nextSequence < 0
    ) {
      errors.push(
        createValidationError(
          ValidationErrorCode.INVALID_INPUT,
          "Sequence number must be a non-negative integer",
          { nextSequence },
        ),
      );
      return { isValid: false, errors };
    }
  }

  if (currentSequence !== undefined && nextSequence !== undefined) {
    if (nextSequence < currentSequence) {
      errors.push(
        createValidationError(
          ValidationErrorCode.SEQUENCE_REGRESSION,
          `Sequence regression detected: incoming sequence ${nextSequence} is strictly less than current ${currentSequence}`,
          { currentSequence, nextSequence },
        ),
      );
      return { isValid: false, errors };
    }
  }

  return { isValid: true, errors: [] };
}

// ---------------------------------------------------------------------------
// Price Boundary Validation
// ---------------------------------------------------------------------------

/**
 * Validates PriceData boundary conditions and constraints.
 */
export function validatePriceData(
  data: unknown,
  bounds?: AssetPriceBounds,
  options?: { maxFutureSkewSeconds?: number },
): ValidationResult {
  const errors: ValidationError[] = [];
  if (!data || typeof data !== "object") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Price data must be a non-null object",
      ),
    );
    return { isValid: false, errors };
  }

  const p = data as Partial<PriceData>;

  // Asset validation
  if (!p.asset || !isSupportedAsset(p.asset)) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_ASSET,
        `Unsupported or missing asset: ${p.asset}`,
      ),
    );
  }

  // Price validation
  if (typeof p.price !== "bigint") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Price must be a bigint",
      ),
    );
  } else {
    if (p.price === 0n) {
      errors.push(
        createValidationError(
          ValidationErrorCode.PRICE_ZERO,
          "Price cannot be zero",
        ),
      );
    } else if (p.price < 0n) {
      errors.push(
        createValidationError(
          ValidationErrorCode.PRICE_NEGATIVE,
          "Price cannot be negative",
        ),
      );
    }

    // Bounds validation
    if (bounds) {
      const minBig = BigInt(Math.floor(bounds.minPrice * 1_000_000));
      const maxBig = BigInt(Math.ceil(bounds.maxPrice * 1_000_000));
      if (p.price < minBig) {
        errors.push(
          createValidationError(
            ValidationErrorCode.PRICE_BELOW_MIN,
            `Price ${p.price} is below minimum allowed bound ${minBig}`,
            { price: p.price, minBound: minBig },
          ),
        );
      }
      if (p.price > maxBig) {
        errors.push(
          createValidationError(
            ValidationErrorCode.PRICE_ABOVE_MAX,
            `Price ${p.price} is above maximum allowed bound ${maxBig}`,
            { price: p.price, maxBound: maxBig },
          ),
        );
      }
    }
  }

  // Timestamp validation
  const maxSkew = options?.maxFutureSkewSeconds ?? 60;
  const nowSec = Math.floor(Date.now() / 1000);
  if (
    typeof p.timestamp !== "number" ||
    !Number.isFinite(p.timestamp) ||
    p.timestamp <= 0
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Timestamp must be a positive number",
      ),
    );
  } else if (p.timestamp > nowSec + maxSkew) {
    errors.push(
      createValidationError(
        ValidationErrorCode.PRICE_STALE,
        `Timestamp is in the future beyond acceptable skew (${p.timestamp} > ${nowSec + maxSkew})`,
        { timestamp: p.timestamp, now: nowSec },
      ),
    );
  }

  // Confidence validation
  if (
    typeof p.confidence !== "number" ||
    !Number.isFinite(p.confidence) ||
    p.confidence < 0 ||
    p.confidence > 100
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Confidence must be a number between 0 and 100",
      ),
    );
  }

  // Source validation
  if (
    !p.source ||
    typeof p.source !== "string" ||
    p.source.trim().length === 0
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Source must be a non-empty string",
      ),
    );
  }

  // Volume24h validation
  if (p.volume24h !== undefined) {
    if (typeof p.volume24h !== "bigint" || p.volume24h < 0n) {
      errors.push(
        createValidationError(
          ValidationErrorCode.INVALID_INPUT,
          "volume24h must be a non-negative bigint",
        ),
      );
    }
  }

  // Signer / Signature pairing
  if (p.signer || p.signature) {
    if (!p.signer || !p.signature) {
      errors.push(
        createValidationError(
          ValidationErrorCode.INVALID_INPUT,
          "Both signer and signature must be present when signed payload is provided",
        ),
      );
    }
  }

  return {
    isValid: errors.length === 0,
    price: errors.length === 0 ? (data as PriceData) : undefined,
    errors,
  };
}

/**
 * Validates RawPriceData boundary conditions and numeric values.
 */
export function validateRawPriceData(
  data: unknown,
  bounds?: AssetPriceBounds,
): ValidationResult {
  const errors: ValidationError[] = [];
  if (!data || typeof data !== "object") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Raw price data must be a non-null object",
      ),
    );
    return { isValid: false, errors };
  }

  const p = data as Partial<RawPriceData>;

  if (!p.asset || !isSupportedAsset(p.asset)) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_ASSET,
        `Unsupported or missing asset: ${p.asset}`,
      ),
    );
  }

  if (typeof p.price !== "number" || !Number.isFinite(p.price)) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Raw price must be a finite number",
      ),
    );
  } else {
    if (p.price === 0) {
      errors.push(
        createValidationError(
          ValidationErrorCode.PRICE_ZERO,
          "Raw price cannot be zero",
        ),
      );
    } else if (p.price < 0) {
      errors.push(
        createValidationError(
          ValidationErrorCode.PRICE_NEGATIVE,
          "Raw price cannot be negative",
        ),
      );
    }

    if (bounds) {
      if (p.price < bounds.minPrice) {
        errors.push(
          createValidationError(
            ValidationErrorCode.PRICE_BELOW_MIN,
            `Raw price ${p.price} below min ${bounds.minPrice}`,
          ),
        );
      }
      if (p.price > bounds.maxPrice) {
        errors.push(
          createValidationError(
            ValidationErrorCode.PRICE_ABOVE_MAX,
            `Raw price ${p.price} above max ${bounds.maxPrice}`,
          ),
        );
      }
    }
  }

  if (
    typeof p.timestamp !== "number" ||
    !Number.isFinite(p.timestamp) ||
    p.timestamp <= 0
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Timestamp must be a positive number",
      ),
    );
  }

  if (
    !p.source ||
    typeof p.source !== "string" ||
    p.source.trim().length === 0
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Source must be a non-empty string",
      ),
    );
  }

  return {
    isValid: errors.length === 0,
    errors,
  };
}

/**
 * Validates AggregatedPrice structural integrity and source boundaries.
 */
export function validateAggregatedPrice(data: unknown): ValidationResult {
  const errors: ValidationError[] = [];
  if (!data || typeof data !== "object") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Aggregated price must be an object",
      ),
    );
    return { isValid: false, errors };
  }

  const p = data as Partial<AggregatedPrice>;

  if (!p.asset || !isSupportedAsset(p.asset)) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_ASSET,
        `Invalid asset ${p.asset}`,
      ),
    );
  }

  if (typeof p.price !== "bigint" || p.price <= 0n) {
    errors.push(
      createValidationError(
        ValidationErrorCode.PRICE_ZERO,
        "Aggregated price must be positive bigint",
      ),
    );
  }

  if (!Array.isArray(p.sources) || p.sources.length === 0) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Sources array cannot be empty",
      ),
    );
  } else {
    for (const s of p.sources) {
      const v = validatePriceData(s);
      if (!v.isValid) {
        errors.push(...v.errors);
      }
    }
  }

  if (
    typeof p.confidence !== "number" ||
    p.confidence < 0 ||
    p.confidence > 100
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "Confidence must be between 0 and 100",
      ),
    );
  }

  if (p.sequence !== undefined) {
    if (
      typeof p.sequence !== "number" ||
      !Number.isInteger(p.sequence) ||
      p.sequence < 0
    ) {
      errors.push(
        createValidationError(
          ValidationErrorCode.INVALID_INPUT,
          "Sequence must be non-negative integer",
        ),
      );
    }
  }

  return {
    isValid: errors.length === 0,
    errors,
  };
}

// ---------------------------------------------------------------------------
// Policy Boundaries Validation
// ---------------------------------------------------------------------------

export function validateFreshnessPolicy(policy: unknown): ValidationResult {
  const errors: ValidationError[] = [];
  if (!policy || typeof policy !== "object") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "FreshnessPolicy must be a non-null object",
      ),
    );
    return { isValid: false, errors };
  }
  const fp = policy as Partial<FreshnessPolicy>;
  if (
    typeof fp.maxStalenessSeconds !== "number" ||
    fp.maxStalenessSeconds <= 0 ||
    !Number.isFinite(fp.maxStalenessSeconds)
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "maxStalenessSeconds must be positive",
      ),
    );
  }
  if (
    typeof fp.maxDeviationPercent !== "number" ||
    fp.maxDeviationPercent < 0 ||
    fp.maxDeviationPercent > 100
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "maxDeviationPercent must be between 0 and 100",
      ),
    );
  }
  if (typeof fp.requireFresh !== "boolean") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "requireFresh must be boolean",
      ),
    );
  }
  if (typeof fp.fallbackOnStale !== "boolean") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "fallbackOnStale must be boolean",
      ),
    );
  }
  return { isValid: errors.length === 0, errors };
}

export function validateFallbackPolicy(policy: unknown): ValidationResult {
  const errors: ValidationError[] = [];
  if (!policy || typeof policy !== "object") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "FallbackPolicy must be a non-null object",
      ),
    );
    return { isValid: false, errors };
  }
  const fp = policy as Partial<FallbackPolicy>;
  if (typeof fp.enabled !== "boolean") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "enabled must be boolean",
      ),
    );
  }
  if (fp.fallbackOrder !== "priority" && fp.fallbackOrder !== "round-robin") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "fallbackOrder must be 'priority' or 'round-robin'",
      ),
    );
  }
  if (
    typeof fp.minSources !== "number" ||
    !Number.isInteger(fp.minSources) ||
    fp.minSources < 1
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "minSources must be integer >= 1",
      ),
    );
  }
  if (
    typeof fp.maxFallbackAttempts !== "number" ||
    !Number.isInteger(fp.maxFallbackAttempts) ||
    fp.maxFallbackAttempts < 0
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "maxFallbackAttempts must be integer >= 0",
      ),
    );
  }
  return { isValid: errors.length === 0, errors };
}

export function validateRecoveryPolicy(policy: unknown): ValidationResult {
  const errors: ValidationError[] = [];
  if (!policy || typeof policy !== "object") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "RecoveryPolicy must be a non-null object",
      ),
    );
    return { isValid: false, errors };
  }
  const rp = policy as Partial<RecoveryPolicy>;
  if (typeof rp.enabled !== "boolean") {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "enabled must be boolean",
      ),
    );
  }
  if (typeof rp.timeoutSeconds !== "number" || rp.timeoutSeconds <= 0) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "timeoutSeconds must be positive number",
      ),
    );
  }
  if (
    rp.statePersistence !== "none" &&
    rp.statePersistence !== "memory" &&
    rp.statePersistence !== "redis"
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        "statePersistence must be 'none', 'memory', or 'redis'",
      ),
    );
  }
  return { isValid: errors.length === 0, errors };
}

// ---------------------------------------------------------------------------
// Authorization Boundary Validation
// ---------------------------------------------------------------------------

export function validateAuthorization(authContext: {
  apiKey?: string;
  expectedApiKey?: string;
}): ValidationResult {
  const errors: ValidationError[] = [];
  if (!authContext.expectedApiKey) {
    return { isValid: true, errors: [] };
  }
  if (
    !authContext.apiKey ||
    authContext.apiKey !== authContext.expectedApiKey
  ) {
    errors.push(
      createValidationError(
        ValidationErrorCode.UNAUTHORIZED,
        "Caller is not authorized to perform oracle operation",
      ),
    );
    return { isValid: false, errors };
  }
  return { isValid: true, errors: [] };
}
