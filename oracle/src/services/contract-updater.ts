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
}

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
