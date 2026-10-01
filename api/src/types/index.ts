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
  version: typeof AMM_EVENT_TOPIC_VERSION;
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
 * The interfaces above are compile-time only. Values crossing the API
 * boundary (JSON bodies, Soroban RPC logs, cache entries) are `unknown`
 * at runtime and must be validated before use. These guards enforce the
 * invariants that the type system cannot enforce on untrusted input:
 *
 * - Addresses are canonical 56-character Stellar StrKey encodings.
 * - Amounts are non-empty decimal strings with no sign, exponent, or leading
 *   zeroes, and are non-zero.
 * - AMM event topics are the exact module/version and a known kind.
 * - AMM event payloads are self-consistent: event kind matches the data
 *   shape, schema_version is 1, timestamps are non-negative integers, and
 *   assets on both sides of a swap/liquidity op are distinct.
 * - Transaction status values are from the closed set of `TransactionStatus`.
 *
 * The guards are pure and stateless, so they are safe to call concurrently
 * and can be retried without side effects. They never include the offending
 * value in error messages to avoid leaking secrets or PIIs.
 */

const STELLAR_ADDRESS_RE = /^G[A-Z2-7]{55}$/;
const AMM_KINDS: readonly AmmEventKind[] = [
  'swap',
  'add_liquidity',
  'remove_liquidity',
];

const DECIMAL_AMOUNT_RE = /^(0|[1-9][0-9]*)$/;

const MAX_AMOUNT_LITERALS = 256;

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

function isStringField(value: unknown): value is string {
  return typeof value === 'string' && value.length > 0;
}

export function isStellarAddress(value: unknown): value is string {
  return typeof value === 'string' && STELLAR_ADDRESS_RE.test(value);
}

export function isDecimalAmount(value: unknown): value is string {
  if (typeof value !== 'string') return false;
  if (value.length === 0 || value.length > MAX_AMOUNT_LITERALS) return false;
  if (!DECIMAL_AMOUNT_RE.test(value)) return false;
  return true;
}

export function isPositiveAmount(value: unknown): value is string {
  return isDecimalAmount(value) && value !== '0';
}

export function isAmmEventKind(value: unknown): value is AmmEventKind {
  return typeof value === 'string' && (AMM_KINDS as readonly string[]).includes(value);
}

export function isAmmEventTopic(value: unknown): value is AmmEventTopic {
  if (!isPlainObject(value)) return false;
  return (
    value.module === AMM_EVENT_TOPIC_MODULE &&
    value.version === AMM_EVENT_TOPIC_VERSION &&
    isAmmEventKind(value.kind)
  );
}

export function isAmmEventV1(value: unknown): value is AmmEventV1
{
  if (!isPlainObject(value)) return false;
  if (value.schema_version !== 1) return false;
  if (!isAmmEventKind(value.event)) return false;
  if (!isStellarAddress(value.user)) return false;
  if (!isStellarAddress(value.pool)) return false;
  if (typeof value.timestamp !== 'number') return false;
  if (!Number.isInteger(value.timestamp)) return false;
  if (value.timestamp < 0) return false;

  switch (value.event) {
    case 'swap':
      return (
        isStellarAddress(value.asset_in) &&
        isStellarAddress(value.asset_out) &&
        value.asset_in !== value.asset_out &&
        isPositiveAmount(value.amount_in) &&
        isPositiveAmount(value.amount_out)
      );
    case 'add_liquidity':
      return (
        isStellarAddress(value.asset_a) &&
        isStellarAddress(value.asset_b) &&
        value.asset_a !== value.asset_b &&
        isPositiveAmount(value.amount_a) &&
        isPositiveAmount(value.amount_b) &&
        isPositiveAmount(value.shares_minted)
      );
    case 'remove_liquidity':
      return (
        isStellarAddress(value.asset_a) &&
        isStellarAddress(value.asset_b) &&
        value.asset_a !== value.asset_b &&
        isPositiveAmount(value.amount_a) &&
        isPositiveAmount(value.amount_b) &&
        isPositiveAmount(value.shares_burned)
      );
    default:
      return false;
  }
}

export function isAmmEventDecodeResult(value: unknown): value is AmmEventDecodeResult {
  if (!isPlainObject(value)) return false;
  if (!isAmmEventTopic(value.topic)) return false;
  if (!isAmmEventV1(value.data)) return false;
  return value.topic.kind === value.data.event;
}

export function isTransactionStatus(value: unknown): value is TransactionStatus {
  return (
    typeof value === 'string' &&
    Object.values(TransactionStatus as Record<string, string>).includes(value)
  );
}

export function isTransactionResponse(value: unknown): value is TransactionResponse {
  if (!isPlainObject(value)) return false;
  if (typeof value.success !== 'boolean') return false;
  if (!isTransactionStatus(value.status)) return false;
  if (value.status === TransactionStatus.NOT_FOUND) return false;
  if (value.transactionHash !== undefined && !isStringField(value.transactionHash)) {
    return false;
  }
  if (value.message !== undefined && typeof value.message !== 'string') return false;
  if (value.error !== undefined && typeof value.error !== 'string') return false;
  if (value.ledger !== undefined) {
    if (typeof value.ledger !== 'number') return false;
    if (!Number.isInteger(value.ledger)) return false;
    if (value.ledger < 0) return false;
  }
  return true;
}

export function isPositionResponse(value: unknown): value is PositionResponse {
  if (!isPlainObject(value)) return false;
  if (!isStellarAddress(value.userAddress)) return false;
  if (!isDecimalAmount(value.collateral)) return false;
  if (!isDecimalAmount(value.debt)) return false;
  if (!isDecimalAmount(value.borrowInterest)) return false;
  if (typeof value.lastAccrualTime !== 'number') return false;
  if (!Number.isInteger(value.lastAccrualTime)) return false;
  if (value.lastAccrualTime < 0) return false;
  if (value.collateralRatio !== undefined && !isDecimalAmount(value.collateralRatio)) {
    return false;
  }
  return true;
}

export function isHealthCheckResponse(value: unknown): value is HealthCheckResponse {
  if (!isPlainObject(value)) return false;
  if (value.status !== 'healthy' && value.status !== 'unhealthy') return false;
  if (typeof value.timestamp !== 'string') return false;
  if (!Number.isFinite(Date.parse(value.timestamp as string))) return false;
  if (!isPlainObject(value.services)) return false;
  const services = value.services as Record<string, unknown>;
  if (typeof services.horizon !== 'boolean') return false;
  if (typeof services.sorobanRpc !== 'boolean') return false;
  if (services.sorobanBreaker !== undefined) {
    if (!isPlainObject(services.sorobanBreaker)) return false;
    const breaker = services.sorobanBreaker as Record<string, unknown>;
    if (typeof breaker.state !== 'string') return false;
    if (typeof breaker.windowMs !== 'number') return false;
    if (typeof breaker.total !== 'number') return false;
    if (typeof breaker.failures !== 'number') return false;
    if (typeof breaker.failureRate !== 'number') return false;
    if (breaker.failureRate < 0 || breaker.failureRate > 1) return false;
  }
  return true;
}

export function assertStellarAddress(value: unknown, field = 'address'): asserts value is string {
  if (!isStellarAddress(value)) {
    throw new TypeError(`Invalid Stellar address for ${field}`);
  }
}

export function assertPositiveAmount(value: unknown, field = 'amount'): asserts value is string {
  if (!isPositiveAmount(value)) {
    throw new TypeError(`Invalid positive amount for ${field}`);
  }
}
