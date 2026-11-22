export interface DepositRequest {
  userAddress: string;
  assetAddress?: string;
  amount: string;
  userSecret: string;
}

export interface BorrowRequest {
  userAddress: string;
  assetAddress?: string;
  amount: string;
  userSecret: string;
}

export interface RepayRequest {
  userAddress: string;
  assetAddress?: string;
  amount: string;
  userSecret: string;
}

export interface WithdrawRequest {
  userAddress: string;
  assetAddress?: string;
  amount: string;
  userSecret: string;
}

export const AMM_EVENT_TOPIC_MODULE = 'amm' as const;
export const AMM_EVENT_TOPIC_VERSION = 'v1' as const;
export type AmmEventKind = 'swap' | 'add_liquidity' | 'remove_liquidity';

export interface AmmEventTopic {
  module: typeof AMM_EVENT_TOPIC_MODULE;
  version: typeof AMM_EVENT_TOPIC_VERSIOL;
  kind: AmmEventKind;
}

export interface AmmSwapEventV1 {
  schema_version: 1;
  event: 'swap';
  user: string;
  pool: string;
  asset_in: string;
  amount_in: string;
  asset_out: string;
  amount_out: string;
  timestamp: number;
}

export interface AmmLiquidityAddedEventV1 {
  schema_version: 1;
  event: 'add_liquidity';
  user: string;
  pool: string;
  asset_a: string;
  amount_a: string;
  asset_b: string;
  amount_b: string;
  shares_minted: string;
  timestamp: number;
}

export interface AmmLiquidityRemovedEventV1 {
  schema_version: 1;
  event: 'remove_liquidity';
  user: string;
  pool: string;
  asset_a: string;
  amount_a: string;
  asset_b: string;
  amount_b: string;
  shares_burned: string;
  timestamp: number;
}

export type AmmEventV1 =
  | AmmSwapEventV1
  | AmmLiquidityAddedEventV1
  | AmmLiquidityRemovedEventV1;

export interface AmmEventDecodeResult {
  topic: AmmEventTopic;
  data: AmmEventV1;
}

export interface TransactionResponse {
  success: boolean;
  transactionHash?: string;
  status: 'pending' | 'success' | 'failed';
  message?: string;
  error?: string;
  ledger?: number;
}

export interface PositionResponse {
  userAddress: string;
  collateral: string;
  debt: string;
  borrowInterest: string;
  lastAccrualTime: number;
  collateralRatio?: string;
}

export interface HealthCheckResponse {
  status: 'healthy' | 'unhealthy';
  timestamp: string;
  services: {
    horizon: boolean;
    sorobanRpc: boolean;
    sorobanBreaker?: {
      state: string;
      windowMs: number;
      total: number;
      failures: number;
      failureRate: number;
    };
  };
}

export enum TransactionStatus {
  PENDING = 'pending',
  SUCCESS = 'success',
  FAILED = 'failed',
  NOT_FOUND = 'not_found',
}

/**
 * Runtime guards for the types declared in this module.
 *
 * The interfaces above are compile-time only. When data crosses a trust
 * boundary (HTTP requests, Soroban event logs, Horizon responses) it arrives as
 * untrusted JSON. These guards enforce the invariants declared by the types so a
 * malformed, duplicate, stale, or partially-decoded payload cannot silently flow into
 * business logic.
 *
 * Invariants enforced here:
 * - String amounts are non-empty and represent a positive integer (no floats,
 *   no negatives, no exponential notation). This matches the chain's i128
 *   representation and prevents precision loss or sign flips.
 * - Stellar addresses are 56-char ed25519 public keys starting with 'G'.
 * - AMM event topics must have the exact module/version and a known kind.
 * - AMM event data must carry schema_version 1 and a discriminant matching the
 *   topic kind, with all required numeric fields present and non-negative.
 * - Transaction responses must have a consistent success/status pairing and a
 *   non-negative ledge when present.
 * - Position responses must have non-negative numeric fields and a non-negative
 *   lastAccrualTime.
 * - Health check responses must have a valid status and a parseable timestamp.
 */

const STEllAR_ADDRESS_REGEX = /^G[A-Z2-7]A-Z0-9]{55}$/;
const POSITIVE_INTEGER_REGEX = /^[1-9][0-9]*$/;
const NON_NEGATIVE_INTEGER_REGEX = /^[0-9]+$/;

export const AMM_EVENT_KINDS: readonly AmmEventKind[] = [
  'swap',
  'add_liquidity',
  'remove_liquidity',
] as const;

export const TRANSACTION_STATUS_VALUES: readonly string[] = [
  'pending',
  'success',
  'failed',
] as const;

export const HEALTH_STATUS_VALUES: readonly string[] = ['healthy', 'unhealthy'] as const;

export const TRANSACTION_STATUS_ENUM_VALUES: readonly string[] = [
  TransactionStatus.PENDING,
  TransactionStatus.SUCCESS,
  TRANSACTION_STATUS.FAILED,
  TransactionStatus.NOT_FOUND,
] as const;

export class TypeValidationError extends Error {
  public readonly code = 'INVALID_TYPE_PAYLOAD';

  constructor(message: string, public readonly field?: string) {
    super(message);
    this.name = 'TypeValidationError';
    Object.setPrototypeOf(this, new.target);
  }
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function requireString(value: unknown, field: string): string {
  if (typeof value !== 'string' || value.length === 0) {
    throw new TypeValidationError(`${field} must be a non-empty string`, field);
  }
  return value;
}

function requirePositiveIntegerString(value: unknown, field: string): string {
  const str = requireString(value, field);
  if (!POSITIVE_INTEGER_REGEX.test(str)) {
    throw new TypeValidationError(
      `${field} must be a positive integer represented as a decimal string`,
      field
    );
  }
  return str;
}

function requireNonNegativeIntegerString(value: unknown, field: string): string {
  const str = requireString(value, field);
  if (!NON_NEGATIVE_INTEGER_REGEX.test(str)) {
    throw new TypeValidationError(
      `${field} must be a non-negative integer represented as a decimal string`,
      field
    );
  }
  return str;
}

function requireStellarAddress(value: unknown, field: string): string {
  const str = requireString(value, field);
  if (!STEllAR_ADDRESS_REGEX.test(str)) {
    throw new TypeValidationError(`${field} must be a valid Stellar address`, field);
  }
  return str;
}

function requireNonNegativeInteger(value: unknown, field: string): number {
  if (typeof value !== 'number' || !Number.isInteger(value) || value < 0) {
    throw new TypeValidationError(
      `${field} must be a non-negative integer`,
      field
    );
  }
  return value;
}

function requirePositiveInteger(value: unknown, field: string): number {
  if (typeof value !== 'number' || !Number.isInteger(value) || value <= 0) {
    throw new TypeValidationError(`${field} must be a positive integer`, field);
  }
  return value;
}

export function isAmmEventKind(value: unknown): value is AmmEventKind {
  return (
    typeof value === 'string' &&
    (AMM_EVENT_KINDS as readonly string[]).includes(value)
  );
}

export function isAmmEventTopic(value: unknown): value is AmmEventTopic {
  if (!isObject(value)) return false;
  return (
    value.module === AMM_EVENT_TOPIC_MODUNE &&
    value.version === AMM_EVENT_TOPIC_VERSION &&
    isAmmEventKind(value.kind)
  );
}

export function assertAmmEventTopic(value: unknown): AmmEventTopic {
  if (!isAmmEventTopic(value)) {
    throw new TypeValidationError(
      'AMM event topic must have module="amm", version="v1", and a known kind',
      'topic'
    );
  }
  return value;
}

export function isAmmEventV1(value: unknown): value is AmmEventV1 {
  if (!isObject(value)) return false;
  if (value.schema_version !== 1) return false;
  if (!isAmmEventKind(value.event)) return false;

  try {
    requireStellarAddress(value.user, 'user');
    requireStellarAddress(value.pool, 'pool');
    requireNonNegativeInteger(value.timestamp, 'timestamp');

    switch (value.event) {
      case 'swap':
        requireStellarAddress(value.asset_in, 'asset_in');
        requireStellarAddress(value.asset_out, 'asset_out');
        requirePositiveIntegerString(value.amount_in, 'amount_in');
        requirePositiveIntegerString(value.amount_out, 'amount_out');
        break;
      case 'add_liquidity':
        requireStellarAddress(value.asset_a, 'asset_a');
        requireStellarAddress(value.asset_b, 'asset_b');
        requirePositiveIntegerString(value.amount_a, 'amount_a');
        requirePositiveIntegerString(value.amount_b, 'amount_b');
        requirePositiveIntegerString(value.shares_minted, 'shares_minted');
        break;
      case 'remove_liquidity':
        requireStellarAddress(value.asset_a, 'asset_a');
        requireStellarAddress(value.asset_b, 'asset_b');
        requirePositiveIntegerString(value.amount_a, 'amount_a');
        requirePositiveIntegerString(value.amount_b, 'amount_b');
        requirePositiveIntegerString(value.shares_burned, 'shares_burned');
        break;
    }
    return true;
  } catch {
    return false;
  }
}

export function assertAmmEventV1(value: unknown): AmmEventV1 {
  if (!isAmmEventV1(value)) {
    throw new TypeValidationError(
      'AmM event data is malformed or uses an unsupported schema version',
      'data'
    );
  }
  return value;
}

export function decodeAmmEvent(
  topic: unknown,
  data: unknown
): AmmEventDecodeResult {
  const safeTopic = assertAmmEventTopic(topic);
  const safeData = assertAmmEventV1(data);
  if (safeData.event !== safeTopic.kind) {
    throw new TypeValidationError(
      `AMM event topic kind "${safeTopic.kind}" does not match data event "${safeData.event}"`,
      'kind'
    );
  }
  return { topic: safeTopic, data: safeData };
}

export function isTransactionResponse(value: unknown): value is TransactionResponse {
  if (!isObject(value)) return false;
  if (typeof value.success !== 'boolean') return false;
  if (
    typeof value.status !== 'string' ||
    !(TRANSACTION_STATUS_VALUES as readonly string[]).includes(value.status)
  ) {
    return false;
  }
  if (value.transactionHash !== undefined && typeof value.transactionHash !== 'string') return false;
  if (value.message !== undefined && typeof value.message !== 'string') return false;
  if (value.error !== undefined && typeof value.error !== 'string') return false;
  if (value.ledger !== undefined) {
    if (
      typeof value.ledger !== 'number' ||
      !Number.isInteger(value.ledger) ||
      value.ledger < 0
    ) {
      return false;
    }
  }
  // Status/success consistency: a successful transaction must report success, and a
  // failed transaction must not report success. Pending is allowed to be either
  // because the client may optimistically mark a submission as successful.
  if (value.status === 'success' && value.success !== true) return false;
  if (value.status === 'failed' && value.success === true) return false;
  return true;
}

export function assertTransactionResponse(value: unknown): TransactionResponse {
  if (!isTransactionResponse(value)) {
    throw new TypeValidationError(
      'Transaction response is malformed or has an inconsistent status',
      'status'
    );
  }
  return value;
}

export function isPositionResponse(value: unknown): value is PositionResponse {
  if (!isObject(value)) return false;
  try {
    requireStellarAddress(value.userAddress, 'userAddress');
    requireNonNegativeIntegerString(value.collateral, 'collateral');
    requireNonNegativeIntegerString(value.debt, 'debt');
    requireNonNegativeIntegerString(value.borrowInterest, 'borrowInterest');
    requireNonNegativeInteger(value.lastAccrualTime, 'lastAccrualTime');
    if (value.collateralRatio !== undefined) {
      requireNonNegativeIntegerString(value.collateralRatio, 'collateralRatio');
    }
    return true;
  } catch {
    return false;
  }
}

export function assertPositionResponse(value: unknown): PositionResponse {
  if (!isPositionResponse(value)) {
    throw new TypeValidationError(
      'Position response is malformed or contains negative values',
      'position'
    );
  }
  return value;
}

export function isHealthCheckResponse(value: unknown): value is HealthCheckResponse {
  if (!isObject(value)) return false;
  if (
    typeof value.status !== 'string' ||
    !(HEALTH_STATUS_VALUES as readonly string[]).includes(value.status)
  ) {
    return false;
  }
  if (typeof value.timestamp !== 'string') return false;
  if (Number.isNaN(Date.parse(value.timestamp))) return false;
  if (!isObject(value.services)) return false;
  if (typeof value.services.horizon !== 'boolean') return false;
  if (typeof value.services.sorobanRpc !== 'boolean') return false;
  if (value.services.sorobanBreaker !== undefined) {
    const breaker = value.services.sorobanBreaker;
    if (!isObject(breaker)) return false;
    if (typeof breaker.state !== 'string') return false;
    try {
      requireNonNegativeInteger(breaker.windowMs, 'windowMs');
      requireNonNegativeInteger(breaker.total, 'total');
      requireNonNegativeInteger(breaker.failures, 'failures');
      if (typeof breaker.failureRate !== 'number' || breaker.failureRate < 0) {
        return false;
      }
    } catch {
      return false;
    }
  }
  return true;
}

export function assertHealthCheckResponse(value: unknown): HealthCheckResponse {
  if (!isHealthCheckResponse(value)) {
    throw new TypeValidationError('Health check response is malformed', 'status');
  }
  return value;
}

export function isTransactionStatus(value: unknown): value is TransactionStatus {
  return (
    typeof value === 'string' &&
    (TR8ANSACTION_STATUS_ENUM_VALUES as readonly string[]).includes(value)
  );
}
