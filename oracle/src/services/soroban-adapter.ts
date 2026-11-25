/**
 * Soroban RPC Adapter for ContractUpdater
 *
 * Builds, signs, prepares, submits, and polls Soroban `set_price` transactions
 * and reads on-chain price records via `get_price_record`.
 *
 * Invariants:
 *   - I-2: ADMIN_SECRET_KEY is never logged or included in any error message.
 *   - I-4: Transaction confirmation polling is strictly bounded by txPollTimeoutMs.
 */

import {
  Address,
  BASE_FEE,
  Contract,
  Keypair,
  Networks,
  SorobanRpc,
  StrKey,
  TransactionBuilder,
  nativeToScVal,
  scValToNative,
  xdr,
} from '@stellar/stellar-sdk';
import type { Account, FeeBumpTransaction, Transaction } from '@stellar/stellar-sdk';
import { scalePrice } from '../config.js';
import type { ContractUpdaterAdapter, LatestUpdate, UpdateSubmission } from './contract-updater.js';

export const DEFAULT_TX_POLL_TIMEOUT_MS = 60_000;
export const DEFAULT_POLL_INTERVAL_MS = 1_000;
const MAX_ASSET_SYMBOL_BYTES = 12;
const STELLAR_SECRET_REGEX = /S[A-Z2-7]{55}/g;
const ORACLE_PAYLOAD_PREFIX = Buffer.from('StellarLendOracle:v2', 'utf8');

export type SorobanAdapterErrorCode =
  | 'INVALID_ASSET'
  | 'TX_FAILED'
  | 'TX_POLL_TIMEOUT'
  | 'TX_SEND_ERROR'
  | 'RPC_ERROR';

export class SorobanAdapterError extends Error {
  readonly code: SorobanAdapterErrorCode;
  readonly retryable: boolean;

  constructor(code: SorobanAdapterErrorCode, message: string) {
    super(message);
    this.name = 'SorobanAdapterError';
    this.code = code;
    this.retryable = code !== 'INVALID_ASSET';
  }
}

export interface SorobanRpcServerLike {
  getAccount(address: string): Promise<Account>;
  prepareTransaction(
    tx: Transaction | FeeBumpTransaction,
  ): Promise<Transaction | FeeBumpTransaction>;
  sendTransaction(
    tx: Transaction | FeeBumpTransaction,
  ): Promise<{
    status: string;
    hash: string;
    errorResult?: unknown;
  }>;
  getTransaction(hash: string): Promise<{
    status: string;
    resultXdr?: unknown;
    returnValue?: xdr.ScVal;
  }>;
  simulateTransaction(tx: Transaction | FeeBumpTransaction): Promise<{
    result?: { retval?: xdr.ScVal };
    error?: string;
  }>;
}

export interface SorobanAdapterConfig {
  network: 'testnet' | 'mainnet';
  rpcUrl: string;
  contractId: string;
  adminSecretKey: string;
  txPollTimeoutMs?: number;
  pollIntervalMs?: number;
  assetAddresses?: Record<string, string>;
  server?: SorobanRpcServerLike;
}

const sleep = (ms: number): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, ms));

export function redactSecret(message: string, secret?: string): string {
  let sanitized = message;
  if (secret && secret.length > 0) {
    sanitized = sanitized.split(secret).join('[REDACTED]');
  }
  return sanitized.replace(STELLAR_SECRET_REGEX, '[REDACTED]');
}

function toTimestampSeconds(observedAt: number): bigint {
  // ContractUpdater uses millisecond timestamps; lending contract uses u64 seconds.
  const seconds = observedAt > 1e12 ? Math.floor(observedAt / 1000) : Math.floor(observedAt);
  return BigInt(Math.max(0, seconds));
}

function toScaledPrice(price: number | bigint): bigint {
  if (typeof price === 'bigint') {
    return price;
  }
  return scalePrice(price);
}

export class SorobanAdapter implements ContractUpdaterAdapter {
  private readonly server: SorobanRpcServerLike;
  private readonly adminKeypair: Keypair;
  private readonly adminSecretKey: string;
  private readonly contractId: string;
  private readonly networkPassphrase: string;
  private readonly txPollTimeoutMs: number;
  private readonly pollIntervalMs: number;
  private readonly assetAddresses: Record<string, string>;

  constructor(config: SorobanAdapterConfig) {
    this.adminSecretKey = config.adminSecretKey;
    this.contractId = config.contractId;
    this.networkPassphrase =
      config.network === 'mainnet' ? Networks.PUBLIC : Networks.TESTNET;
    this.txPollTimeoutMs = config.txPollTimeoutMs ?? DEFAULT_TX_POLL_TIMEOUT_MS;
    this.pollIntervalMs = config.pollIntervalMs ?? DEFAULT_POLL_INTERVAL_MS;
    this.assetAddresses = config.assetAddresses ?? {};

    try {
      this.adminKeypair = Keypair.fromSecret(config.adminSecretKey);
    } catch (error) {
      const msg = this.sanitizeError(error, 'Invalid admin secret key');
      throw new SorobanAdapterError('RPC_ERROR', msg);
    }

    this.server =
      config.server ??
      (new SorobanRpc.Server(config.rpcUrl) as unknown as SorobanRpcServerLike);
  }

  getAdminPublicKey(): string {
    return this.adminKeypair.publicKey();
  }

  private sanitizeError(error: unknown, fallback: string): string {
    const raw = error instanceof Error ? error.message : String(error ?? fallback);
    return redactSecret(raw || fallback, this.adminSecretKey);
  }

  private resolveAssetScVal(asset: string): xdr.ScVal {
    if (typeof asset !== 'string' || asset.trim().length === 0) {
      throw new SorobanAdapterError('INVALID_ASSET', 'Asset must be a non-empty string');
    }

    const mapped = this.assetAddresses[asset] ?? asset;
    if (StrKey.isValidContract(mapped) || StrKey.isValidEd25519PublicKey(mapped)) {
      return new Address(mapped).toScVal();
    }

    if (new TextEncoder().encode(mapped).length > MAX_ASSET_SYMBOL_BYTES) {
      throw new SorobanAdapterError(
        'INVALID_ASSET',
        `Invalid asset identifier: exceeds ${MAX_ASSET_SYMBOL_BYTES} bytes and is not a valid Stellar address`,
      );
    }

    return xdr.ScVal.scvSymbol(mapped);
  }

  private signPricePayload(assetScVal: xdr.ScVal, price: bigint, timestamp: bigint): Buffer {
    const priceBuf = Buffer.alloc(16);
    let remaining = price < 0n ? (1n << 128n) + price : price;
    for (let i = 15; i >= 0; i--) {
      priceBuf[i] = Number(remaining & 0xffn);
      remaining >>= 8n;
    }

    const tsBuf = Buffer.alloc(8);
    tsBuf.writeBigUInt64BE(timestamp, 0);

    const payload = Buffer.concat([
      ORACLE_PAYLOAD_PREFIX,
      assetScVal.toXDR(),
      priceBuf,
      tsBuf,
    ]);
    return this.adminKeypair.sign(payload);
  }

  /**
   * Submit a `set_price(caller, asset, price, timestamp, signature)` transaction
   * matching `stellar-lend/contracts/lending/src/lib.rs`.
   */
  async submit(submission: UpdateSubmission): Promise<{ txHash: string }> {
    const assetScVal = this.resolveAssetScVal(submission.asset);
    const scaledPrice = toScaledPrice(submission.price);
    const timestampSeconds = toTimestampSeconds(submission.observedAt);
    const signature = this.signPricePayload(assetScVal, scaledPrice, timestampSeconds);

    try {
      const account = await this.server.getAccount(this.adminKeypair.publicKey());
      const contract = new Contract(this.contractId);
      const callerScVal = new Address(this.adminKeypair.publicKey()).toScVal();

      // Argument order verified against stellar-lend/contracts/lending/src/lib.rs:
      // pub fn set_price(env: Env, caller: Address, asset: Address, price: i128, timestamp: u64, signature: BytesN<64>)
      const operation = contract.call(
        'set_price',
        callerScVal,
        assetScVal,
        nativeToScVal(scaledPrice, { type: 'i128' }),
        nativeToScVal(timestampSeconds, { type: 'u64' }),
        xdr.ScVal.scvBytes(signature),
      );

      const tx = new TransactionBuilder(account, {
        fee: BASE_FEE,
        networkPassphrase: this.networkPassphrase,
      })
        .addOperation(operation)
        .setTimeout(30)
        .build();

      const prepared = await this.server.prepareTransaction(tx);
      prepared.sign(this.adminKeypair);

      const sendResponse = await this.server.sendTransaction(prepared);
      if (sendResponse.status === 'ERROR') {
        const detail = this.sanitizeError(
          sendResponse.errorResult ?? 'Transaction rejected by Soroban RPC',
          'Transaction rejected by Soroban RPC',
        );
        throw new SorobanAdapterError('TX_SEND_ERROR', `sendTransaction failed: ${detail}`);
      }

      return await this.pollTransaction(sendResponse.hash);
    } catch (error) {
      if (error instanceof SorobanAdapterError) {
        throw new SorobanAdapterError(
          error.code,
          redactSecret(error.message, this.adminSecretKey),
        );
      }
      const message = this.sanitizeError(error, 'Soroban RPC submission failed');
      const code: SorobanAdapterErrorCode = message.includes('INVALID_ASSET')
        ? 'INVALID_ASSET'
        : 'RPC_ERROR';
      throw new SorobanAdapterError(code, message);
    }
  }

  private async pollTransaction(txHash: string): Promise<{ txHash: string }> {
    const deadline = Date.now() + this.txPollTimeoutMs;

    while (true) {
      const result = await this.server.getTransaction(txHash);
      if (result.status === SorobanRpc.Api.GetTransactionStatus.SUCCESS || result.status === 'SUCCESS') {
        return { txHash };
      }
      if (result.status === SorobanRpc.Api.GetTransactionStatus.FAILED || result.status === 'FAILED') {
        throw new SorobanAdapterError(
          'TX_FAILED',
          `Soroban transaction ${txHash} failed on-chain`,
        );
      }

      const remaining = deadline - Date.now();
      if (remaining <= 0) {
        throw new SorobanAdapterError(
          'TX_POLL_TIMEOUT',
          `Soroban transaction ${txHash} timed out after ${this.txPollTimeoutMs}ms`,
        );
      }

      await sleep(Math.min(this.pollIntervalMs, remaining));
    }
  }

  /**
   * Read the latest on-chain price record via `get_price_record(asset)`.
   */
  async getLatestUpdate(asset: string): Promise<LatestUpdate | null> {
    const assetScVal = this.resolveAssetScVal(asset);

    try {
      const account = await this.server.getAccount(this.adminKeypair.publicKey());
      const contract = new Contract(this.contractId);
      const operation = contract.call('get_price_record', assetScVal);

      const tx = new TransactionBuilder(account, {
        fee: BASE_FEE,
        networkPassphrase: this.networkPassphrase,
      })
        .addOperation(operation)
        .setTimeout(30)
        .build();

      const sim = await this.server.simulateTransaction(tx);
      const retval = sim?.result?.retval;
      if (!retval || retval.switch() === xdr.ScValType.scvVoid()) {
        return null;
      }

      const decoded = scValToNative(retval) as
        | { price?: bigint | number; timestamp?: bigint | number }
        | null
        | undefined;
      if (!decoded || decoded.price === undefined || decoded.timestamp === undefined) {
        return null;
      }

      const price =
        typeof decoded.price === 'bigint' ? decoded.price : BigInt(decoded.price);
      const tsSeconds = Number(decoded.timestamp);
      return {
        price,
        timestamp: tsSeconds > 1e12 ? tsSeconds : tsSeconds * 1000,
      };
    } catch (error) {
      if (error instanceof SorobanAdapterError) {
        throw error;
      }
      return null;
    }
  }
}
