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
