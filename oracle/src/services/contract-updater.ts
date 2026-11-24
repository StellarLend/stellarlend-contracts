import {
  Keypair,
  Contract,
  SorobanRpc,
  TransactionBuilder,
  Networks,
  xdr,
  Address,
  nativeToScVal,
} from '@stellar/stellar-sdk';
import type { AggregatedPrice, ContractUpdateResult } from '../types/index.js';
import { logger } from '../utils/logger.js';
/**
 * Contract Updater Service
 *
 * Submits price updates through an adapter. Public updatePrice/updatePrices
 * methods return results instead of throwing; submitPriceUpdate remains the
 * lower-level adapter-based method that throws on failure.
 */

import { createHash } from "node:crypto";
import type {
  AggregatedPrice,
  ContractUpdateResult,
  ContractUpdaterOptions,
} from "../types/index.js";

import { createHash } from 'node:crypto';

export const MAX_RETRIES = 3;
export const BASE_MS = 250;
export const CAP_MS = 8000;
export const TIMEOUT_MS = 120000;
export const STALE_MS = 300000;
export const FUTURE_SKEW_MS = 5000;
export const MAX_ASSET_SYMBOL_BYTES = 12;
export const DEFAULT_TX_POLL_TIMEOUT_MS = 60_000;

const TERMINAL_STATUSES = ['FAILED', 'REJECTED', 'CANCELLED'] as const;
const NON_RETRYABLE_CODES = ['PRICE_STALE', 'INVALID_ASSET', 'SOURCE_UNAVAILABLE'] as const;

export interface ContractUpdaterConfig {
  network: 'testnet' | 'mainnet';
  rpcUrl: string;
  contractId: string;
  adminSecretKey: string;
  maxRetries?: number;
  retryDelayMs?: number;
  txPollTimeoutMs?: number;
}

interface SubmitInput {
  asset: string;
  price: number;
  source: string;
  observedAt?: number;
  timestamp?: number;
  idempotencyKey?: string;
}

interface NormalizedRequest {
  asset: string;
  price: number;
  source: string;
  observedAt: number;
  idempotencyKey: string;
}

interface LatestUpdate {
  price: number;
  timestamp: number;
  txHash?: string;
}

interface AdapterSubmitResult {
  txHash: string;
}

export interface UpdateAdapter {
  submit(submission: Submission): Promise<AdapterSubmitResult>;
  getLatestUpdate(asset: string): Promise<LatestUpdate | null>;
}

export type SubmissionStatus = 'PENDING' | 'CONFIRMED' | 'FAILED' | 'REJECTED' | 'CANCELLED';

export interface Submission {
  id: string;
  asset: string;
  price: number;
  source: string;
  observedAt: number;
  createdAt: number;
  status: SubmissionStatus;
  attempts: number;
  error?: string;
  expiresAt: number;
  lastAttemptAt?: number;
  txHash?: string;
}

interface CodedError extends Error { code?: string }

export function calculateJitterDelay(attempt: number, base = BASE_MS, cap = CAP_MS): number {
  const safeAttempt = Number.isFinite(attempt) && attempt >= 0 ? Math.floor(attempt) : 0;
const TERMINAL_STATUSES = ["FAILED", "REJECTED", "CANCELLED"];
const NON_RETRYABLE_CODES = ["PRICE_STALE", "INVALID_ASSET", "SOURCE_UNAVAILABLE"];

interface UpdateRequest {
  asset: string;
  price: number | bigint;
  source: string;
  observedAt: number;
  idempotencyKey?: string;
}

interface NormalizedRequest extends UpdateRequest {
  idempotencyKey: string;
}

interface UpdateSubmission extends NormalizedRequest {
  id: string;
  createdAt: number;
  status: "PENDING" | "CONFIRMED" | "FAILED" | "REJECTED" | "CANCELLED";
  attempts: number;
  error?: string;
  expiresAt: number;
  lastAttemptAt?: number;
  txHash?: string;
}

interface ContractUpdaterAdapter {
  submit(submission: UpdateSubmission): Promise<{ txHash: string }>;
  getLatestUpdate(asset: string): Promise<{
    price: number | bigint;
    timestamp: number;
    txHash?: string;
  } | null>;
}

export type ContractUpdaterConfig = ContractUpdaterOptions;

export function calculateJitterDelay(
  attempt: number,
  base = BASE_MS,
  cap = CAP_MS,
): number {
  const safeAttempt =
    Number.isFinite(attempt) && attempt >= 0 ? Math.floor(attempt) : 0;
  const capped = Math.min(cap, base * Math.pow(2, safeAttempt));
  const ratio = (((safeAttempt + 1) * 9301 + 49297) % 233280) / 233280;
  return Math.floor(capped * ratio);
}

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));
const isRetryable = (error: unknown): boolean => {
  const code = error instanceof Error ? (error as CodedError).code : undefined;
  return !NON_RETRYABLE_CODES.includes(code as typeof NON_RETRYABLE_CODES[number]);
};

export class ContractUpdater {
  private readonly config?: ContractUpdaterConfig;
  private readonly server?: SorobanRpc.Server;
  private readonly adminKeypair?: Keypair;
  private readonly networkPassphrase?: string;
  public readonly subs = new Map<string, Submission>();
  public readonly latest = new Map<string, Submission>();
  private readonly chains = new Map<string, Promise<Submission | void>>();
  private readonly adapter: UpdateAdapter;

  constructor(adapterOrConfig: UpdateAdapter | ContractUpdaterConfig) {
    if ('submit' in adapterOrConfig || 'getLatestUpdate' in adapterOrConfig) {
      this.adapter = adapterOrConfig as UpdateAdapter;
    } else {
      const config = adapterOrConfig;
      this.config = {
        ...config,
        maxRetries: config.maxRetries ?? MAX_RETRIES,
        retryDelayMs: config.retryDelayMs ?? 1000,
        txPollTimeoutMs: config.txPollTimeoutMs ?? DEFAULT_TX_POLL_TIMEOUT_MS,
      };
      this.server = new SorobanRpc.Server(config.rpcUrl);
      this.adminKeypair = Keypair.fromSecret(config.adminSecretKey);
      this.networkPassphrase = config.network === 'testnet' ? Networks.TESTNET : Networks.PUBLIC;
      this.adapter = {
        submit: async () => { throw new Error('no adapter'); },
        getLatestUpdate: async () => null,
      };
    }
const sleep = (ms: number): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, ms));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const isRetryable = (e) => !NON_RETRYABLE_CODES.includes(e?.code);
export const idFor = (r) => createHash('sha256').update(`${r.asset}:${r.price}:${r.source}:${r.observedAt}`).digest('hex');

function isRetryable(error: unknown): boolean {
  if (!error || typeof error !== "object" || !("code" in error)) return true;
  const code = error.code;
  return typeof code !== "string" || !NON_RETRYABLE_CODES.includes(code);
}

function idFor(request: UpdateRequest): string {
  return createHash("sha256")
    .update(
      `${request.asset}:${request.price}:${request.source}:${request.observedAt}`,
    )
    .digest("hex");
}

function validateInput(input: UpdateRequest): void {
  if (!input || typeof input !== "object") throw new Error("invalid input");
  const { asset, price, source } = input;
  if (typeof asset !== "string" || asset.length === 0) {
    throw new Error("INVALID_ASSET");
  }
  if (typeof source !== "string" || source.length === 0) {
    throw new Error("INVALID_SOURCE");
  }
  if (
    (typeof price !== "number" && typeof price !== "bigint") ||
    (typeof price === "number" && !Number.isFinite(price)) ||
    price <= 0
  ) {
    const error = new Error("INVALID_PRICE");
    Object.assign(error, { code: "INVALID_ASSET" });
    throw error;
  }

  async updatePrice(asset: string, price: bigint, timestamp: number): Promise<ContractUpdateResult> {
    if (!asset || asset.trim().length === 0) return this.invalidResult(asset, price, timestamp, 'asset must be a non-empty string');
    if (new TextEncoder().encode(asset).length > MAX_ASSET_SYMBOL_BYTES) return this.invalidResult(asset, price, timestamp, `asset symbol exceeds maximum length of ${MAX_ASSET_SYMBOL_BYTES} bytes`);
    if (price <= 0n) return this.invalidResult(asset, price, timestamp, 'price must be a positive bigint');
    if (!Number.isInteger(timestamp) || timestamp <= 0) return this.invalidResult(asset, price, timestamp, 'timestamp must be a positive integer');

    const maxRetries = this.config?.maxRetries ?? MAX_RETRIES;
    const retryDelayMs = this.config?.retryDelayMs ?? 1000;
    let lastError = 'Unknown error';

    for (let attempt = 1; attempt <= maxRetries; attempt++) {
      try {
        const txHash = await this.submitSorobanPriceUpdate(asset, price, timestamp);
        return { success: true, transactionHash: txHash, asset, price, timestamp };
      } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        lastError = this.redactSecret(message);
        if (attempt < maxRetries) await sleep(retryDelayMs * Math.pow(2, attempt - 1));
      }
    }

    return this.invalidResult(asset, price, timestamp, lastError);
  }

  async updatePrices(prices: AggregatedPrice[]): Promise<ContractUpdateResult[]> {
    const results: ContractUpdateResult[] = [];
    for (const price of prices) {
      results.push(await this.updatePrice(price.asset, price.price, price.timestamp));
      await sleep(100);
function normalizeRequest(input: UpdateRequest): NormalizedRequest {
  const { observedAt } = input;
  if (typeof observedAt !== "number" || !Number.isFinite(observedAt)) {
    throw new Error("INVALID_OBSERVED_AT");
  }
  const idempotencyKey =
    input.idempotencyKey ??
    `${input.asset}:${input.price}:${input.source}:${observedAt}`;
  return { ...input, observedAt, idempotencyKey };
function normalizeRequest(input) {
  const observedAt = 'observedAt' in input ? input.observedAt : input.timestamp;
  if (typeof observedAt !== 'number' || !Number.isFinite(observedAt)) throw new Error('INVALID_OBSERVED_AT');
  return {
    asset: input.asset,
    price: input.price,
    source: input.source,
    observedAt,
    ...(typeof input.idempotencyKey === 'string' ? { idempotencyKey: input.idempotencyKey } : {}),
  };
}

export class ContractUpdater {
  readonly adapter: ContractUpdaterAdapter;
  readonly subs = new Map<string, UpdateSubmission>();
  readonly latest = new Map<string, UpdateSubmission>();
  readonly chains = new Map<string, Promise<void>>();

  constructor(adapter?: ContractUpdaterAdapter) {
    this.adapter = adapter ?? {
      submit: async () => {
        throw new Error("no adapter");
      },
      getLatestUpdate: async () => null,
    };
  }

  async updatePrice(price: AggregatedPrice): Promise<ContractUpdateResult> {
    const result: ContractUpdateResult = {
      success: false,
      asset: typeof price?.asset === "string" ? price.asset : "",
      price: typeof price?.price === "bigint" ? price.price : 0n,
      timestamp:
        typeof price?.timestamp === "number" && Number.isFinite(price.timestamp)
          ? price.timestamp
          : 0,
    };

    try {
      if (!price || typeof price !== "object") {
        throw new Error("invalid price update");
      }

      const source = Array.isArray(price.sources)
        ? price.sources
            .map((item) => item.source)
            .filter((item) => typeof item === "string" && item.length > 0)
            .join(",") || "aggregated"
        : "aggregated";
      const submission = await this.submitPriceUpdate({
        asset: price.asset,
        price: price.price,
        source,
        observedAt: price.timestamp * 1000,
      });

      result.success = submission.status === "CONFIRMED";
      result.transactionHash = submission.txHash;
      result.idempotencyKey = submission.idempotencyKey;
      if (!result.success) {
        result.error = submission.error ?? "Price update was not confirmed";
      }
    } catch (error) {
      result.error = error instanceof Error ? error.message : String(error);
    }

    return result;
  }

  async updatePrices(
    prices: AggregatedPrice[],
  ): Promise<ContractUpdateResult[]> {
    const results: ContractUpdateResult[] = [];
    for (const price of prices) {
      results.push(await this.updatePrice(price));
    }
    return results;
  }

  async submitPriceUpdate(input: SubmitInput): Promise<Submission> {
    this.validateInput(input);
    const req = this.normalizeRequest(input);
    const now = Date.now();
    if (req.observedAt > now + FUTURE_SKEW_MS || req.observedAt < now - STALE_MS) throw new Error('PRICE_STALE');
    return this.enqueue(req.asset, () => this.process(req));
  }

  private invalidResult(asset: string, price: bigint, timestamp: number, error: string): ContractUpdateResult {
    return { success: false, asset, price, timestamp, error };
  }

  private redactSecret(message: string): string {
    const secret = this.config?.adminSecretKey;
    return secret ? message.split(secret).join('[REDACTED]') : message;
  }

  private async submitSorobanPriceUpdate(asset: string, price: bigint, timestamp: number): Promise<string> {
    if (!this.config || !this.server || !this.adminKeypair || !this.networkPassphrase) throw new Error('Soroban adapter is not configured');
    const contract = new Contract(this.config.contractId);
    const adminAddress = new Address(this.adminKeypair.publicKey());
    const operation = contract.call('set_asset_price', adminAddress.toScVal(), xdr.ScVal.scvSymbol(asset), nativeToScVal(price, { type: 'i128' }), nativeToScVal(timestamp, { type: 'u64' }));
    const account = await this.server.getAccount(this.adminKeypair.publicKey());
    const transaction = new TransactionBuilder(account, { fee: '100000', networkPassphrase: this.networkPassphrase }).addOperation(operation).setTimeout(30).build();
    const simulated = await this.server.simulateTransaction(transaction);
    if (SorobanRpc.Api.isSimulationError(simulated)) throw new Error(`Simulation failed: ${String((simulated as { error?: unknown }).error ?? 'unknown')}`);
    if (!SorobanRpc.Api.isSimulationSuccess(simulated)) throw new Error('Simulation did not succeed');
    const prepared = SorobanRpc.assembleTransaction(transaction, simulated).build();
    prepared.sign(this.adminKeypair);
    const response = await this.server.sendTransaction(prepared);
    if (response.status === 'ERROR') throw new Error(`Transaction failed: ${String((response as { errorResult?: unknown }).errorResult ?? 'unknown')}`);

    const deadline = Date.now() + (this.config.txPollTimeoutMs ?? DEFAULT_TX_POLL_TIMEOUT_MS);
    let getResponse = await this.server.getTransaction(response.hash);
    while (getResponse.status === SorobanRpc.Api.GetTransactionStatus.NOT_FOUND && Date.now() < deadline) {
      await sleep(50);
      getResponse = await this.server.getTransaction(response.hash);
    }
    if (getResponse.status === SorobanRpc.Api.GetTransactionStatus.NOT_FOUND) throw new Error('Transaction timed out waiting for confirmation');
    if (getResponse.status === SorobanRpc.Api.GetTransactionStatus.FAILED) throw new Error('Transaction failed on-chain');
    return response.hash;
  }

  async healthCheck(): Promise<boolean> {
    try {
      if (!this.config) return true;
      return !!new Contract(this.config.contractId);
    } catch { return false; }
  }

  getAdminPublicKey(): string {
    if (!this.adminKeypair) throw new Error('Soroban adapter is not configured');
    return this.adminKeypair.publicKey();
  }

  private validateInput(input: SubmitInput): void {
    if (!input || typeof input !== 'object') throw new Error('invalid input');
    if (typeof input.asset !== 'string' || input.asset.length === 0) throw new Error('INVALID_ASSET');
    if (typeof input.source !== 'string' || input.source.length === 0) throw new Error('INVALID_SOURCE');
    if (typeof input.price !== 'number' || !Number.isFinite(input.price) || input.price <= 0) {
      const error = new Error('INVALID_PRICE') as CodedError;
      error.code = 'INVALID_ASSET';
      throw error;
    }
  }

  private normalizeRequest(input: SubmitInput): NormalizedRequest {
    const observedAt = input.observedAt !== undefined ? input.observedAt : input.timestamp;
    if (typeof observedAt !== 'number' || !Number.isFinite(observedAt)) throw new Error('INVALID_OBSERVED_AT');
    return {
      asset: input.asset,
      price: input.price,
      source: input.source,
      observedAt,
      idempotencyKey: input.idempotencyKey ?? `${input.asset}:${input.price}:${input.source}:${observedAt}`,
    };
  }

  private enqueue(asset: string, task: () => Promise<Submission>): Promise<Submission> {
    const previous = this.chains.get(asset) ?? Promise.resolve();
    const next = previous.then(task, task);
    this.chains.set(asset, next.catch(() => undefined));
    return next;
  }

  private async process(req: NormalizedRequest): Promise<Submission> {
    const existing = this.subs.get(req.idempotencyKey);
    if (existing && !TERMINAL_STATUSES.includes(existing.status as typeof TERMINAL_STATUSES[number])) return existing;

    const last = this.latest.get(req.asset);
    if (last?.status === 'CONFIRMED' && last.price === req.price) return last;

    const submission: Submission = {
      id: req.idempotencyKey,
      asset: req.asset,
      price: req.price,
      source: req.source,
      observedAt: req.observedAt,
  async submitPriceUpdate(input: UpdateRequest): Promise<UpdateSubmission> {
    validateInput(input);
    const request = normalizeRequest(input);
    const now = Date.now();
    if (request.observedAt > now + FUTURE_SKEW_MS) {
      throw new Error("PRICE_STALE");
    }
    if (request.observedAt < now - STALE_MS) {
      throw new Error("PRICE_STALE");
    }
    return this.enqueue(request.asset, () => this.process(request));
  }

  enqueue(
    asset: string,
    task: () => Promise<UpdateSubmission>,
  ): Promise<UpdateSubmission> {
    const previous = this.chains.get(asset) ?? Promise.resolve();
    const next = previous.then(task, task);
    this.chains.set(
      asset,
      next.then(
        () => undefined,
        () => undefined,
      ),
    );
    return next;
  }

  async process(
    request: NormalizedRequest,
  ): Promise<UpdateSubmission> {
    const id = request.idempotencyKey || idFor(request);
    const existing = this.subs.get(id);
    if (existing && !TERMINAL_STATUSES.includes(existing.status)) return existing;

    const last = this.latest.get(request.asset);
    if (
      last &&
      last.status === "CONFIRMED" &&
      last.price === request.price
    ) {
      return last;
    }

    const submission: UpdateSubmission = {
      ...request,
      id,
      createdAt: Date.now(),
      status: "PENDING",
      attempts: existing?.status === "FAILED" ? existing.attempts : 0,
      error: existing?.error,
      expiresAt: Date.now() + TIMEOUT_MS,
    };
    this.subs.set(req.idempotencyKey, submission);

    try {
      await this.execute(submission);
      this.latest.set(req.asset, submission);
      return submission;
    } catch (error) {
      if (this.latest.get(req.asset) === submission) this.latest.delete(req.asset);
    this.subs.set(id, submission);

    try {
      await this.execute(submission);
      this.latest.set(request.asset, submission);
      return submission;
    } catch (error) {
      const current = this.latest.get(request.asset);
      if (current === submission) this.latest.delete(request.asset);
      throw error;
    }
  }

  private async execute(submission: Submission): Promise<Submission> {
    while (submission.attempts <= MAX_RETRIES && Date.now() < submission.expiresAt) {
  async execute(
    submission: UpdateSubmission,
  ): Promise<UpdateSubmission> {
    while (
      submission.attempts <= MAX_RETRIES &&
      Date.now() < submission.expiresAt
    ) {
      submission.attempts++;
      submission.lastAttemptAt = Date.now();
      try {
        const latest = await this.adapter.getLatestUpdate(submission.asset);
        if (latest && latest.price === submission.price && latest.timestamp >= submission.observedAt) {
          submission.status = 'CONFIRMED';
          submission.txHash = latest.txHash;
          return submission;
        }
        const response = await this.adapter.submit(submission);
        if (!response || typeof response.txHash !== 'string') {
          const error = new Error('INVALID_ADAPTER_RESPONSE') as CodedError;
          error.code = 'INVALID_ASSET';
          throw error;
        }
        submission.status = 'CONFIRMED';
        submission.txHash = response.txHash;
        return submission;
      } catch (error) {
        submission.error = error instanceof Error ? error.message : String(error);
        const retry = isRetryable(error);
        if (!retry || submission.attempts > MAX_RETRIES || Date.now() >= submission.expiresAt) {
          submission.status = retry ? 'FAILED' : 'REJECTED';
        if (
          latest &&
          latest.price === submission.price &&
          latest.timestamp >= submission.observedAt
        ) {
          submission.status = "CONFIRMED";
          submission.txHash = latest.txHash;
          return submission;
        }

        const response = await this.adapter.submit(submission);
        if (!response || typeof response.txHash !== "string") {
          const error = new Error("INVALID_ADAPTER_RESPONSE");
          Object.assign(error, { code: "INVALID_ASSET" });
          throw error;
        }
        submission.status = "CONFIRMED";
        submission.txHash = response.txHash;
        return submission;
      } catch (error) {
        submission.error =
          error instanceof Error ? error.message : String(error);
        const retry = isRetryable(error);
        if (
          !retry ||
          submission.attempts > MAX_RETRIES ||
          Date.now() >= submission.expiresAt
        ) {
          submission.status = retry ? "FAILED" : "REJECTED";
          throw new Error(submission.error);
        }
        await sleep(calculateJitterDelay(submission.attempts - 1));
      }
    }
    submission.status = 'FAILED';
    throw new Error(submission.error ?? 'timeout');
  }
}

export function createContractUpdater(config: ContractUpdaterConfig): ContractUpdater {
  return new ContractUpdater(config);

    submission.status = "FAILED";
    throw new Error(submission.error ?? "timeout");
  }
}

export function createContractUpdater(
  _options: ContractUpdaterOptions,
): ContractUpdater {
  // The concrete Soroban adapter is intentionally outside this API change.
  void _options;
  return new ContractUpdater();
}
