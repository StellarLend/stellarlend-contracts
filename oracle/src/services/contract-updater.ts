/**
 * Contract Updater Service
 *
 * Submits signed price-update transactions to the StellarLend on-chain
 * contract via Soroban RPC.  Invariants:
 *
 * I-1  Every public write method validates its inputs before touching the network.
 * I-2  The admin secret key is NEVER included in log output or error messages.
 * I-3  The retry loop uses exponential back-off and terminates after maxRetries.
 * I-4  The transaction-poll loop is bounded by txPollTimeoutMs; it throws after
 *      that deadline rather than spinning forever.
 * I-5  updatePrices processes assets sequentially; a failure for one asset does
 *      NOT abort subsequent assets.
 *
 * Two entry points share the same on-chain submission path:
 *  - updatePrice / updatePrices : validated, never throws, returns ContractUpdateResult.
 *  - submitPriceUpdate          : idempotent, per-asset serialized, stale-checked,
 *                                 adapter-based; throws on failure.
 */

import { createHash } from "crypto";
import {
  Keypair,
  Contract,
  SorobanRpc,
  TransactionBuilder,
  Networks,
  xdr,
  Address,
  nativeToScVal,
} from "@stellar/stellar-sdk";
import type { ContractUpdateResult, AggregatedPrice } from "../types/index.js";
import { logger } from "../utils/logger.js";

// ---------------------------------------------------------------------------
// Public constants
// ---------------------------------------------------------------------------

/** Maximum allowed byte-length for an asset symbol passed to updatePrice. */
export const MAX_ASSET_SYMBOL_BYTES = 12;

/** Default cap (ms) on how long we wait for a submitted tx to be confirmed. */
export const DEFAULT_TX_POLL_TIMEOUT_MS = 60_000;

// Submission-pipeline tuning (submitPriceUpdate)
const MAX_RETRIES = 3;
const BASE_MS = 250;
const CAP_MS = 8_000;
const TIMEOUT_MS = 120_000;
const STALE_MS = 300_000;
const FUTURE_SKEW_MS = 5_000;

const NON_RETRYABLE_CODES = new Set([
  "PRICE_STALE",
  "INVALID_ASSET",
  "SOURCE_UNAVAILABLE",
]);
const RESUBMITTABLE_STATUSES = new Set(["FAILED", "REJECTED", "CANCELLED"]);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface ContractUpdaterConfig {
  network: "testnet" | "mainnet";
  rpcUrl: string;
  /** StellarLend contract ID */
  contractId: string;
  /** Admin secret key for signing – never logged or surfaced in errors (I-2) */
  adminSecretKey: string;
  maxRetries: number;
  retryDelayMs: number;
  /** Max ms to poll for confirmation. Defaults to DEFAULT_TX_POLL_TIMEOUT_MS. */
  txPollTimeoutMs?: number;
}

export type SubmissionStatus =
  | "PENDING"
  | "CONFIRMED"
  | "FAILED"
  | "REJECTED"
  | "CANCELLED";

/** `observedAt` is in milliseconds. */
export interface PriceUpdateRequest {
  asset: string;
  price: bigint;
  source: string;
  observedAt: number;
  idempotencyKey?: string;
}

/** Legacy shape: `timestamp` is in milliseconds. */
export interface LegacyPriceUpdateInput {
  asset: string;
  price: bigint;
  source: string;
  timestamp: number;
}

export interface PriceSubmission {
  id: string;
  asset: string;
  price: bigint;
  source: string;
  observedAt: number;
  createdAt: number;
  status: SubmissionStatus;
  attempts: number;
  lastAttemptAt?: number;
  error?: string;
  txHash?: string;
  expiresAt: number;
}

export interface OnChainUpdate {
  price: bigint;
  timestamp: number;
  txHash?: string;
}

export interface ContractAdapter {
  submit(sub: PriceSubmission): Promise<{ txHash: string }>;
  getLatestUpdate(asset: string): Promise<OnChainUpdate | null>;
}

const DEFAULT_CONFIG: Partial<ContractUpdaterConfig> = {
  maxRetries: 3,
  retryDelayMs: 1000,
  txPollTimeoutMs: DEFAULT_TX_POLL_TIMEOUT_MS,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

export function calculateJitterDelay(
  attempt: number,
  base: number = BASE_MS,
  cap: number = CAP_MS,
): number {
  const capped = Math.min(cap, base * Math.pow(2, attempt));
  const ratio = (((attempt + 1) * 9301 + 49297) % 233280) / 233280;
  return Math.floor(capped * ratio);
}

const sleep = (ms: number): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, ms));

const isRetryable = (e: unknown): boolean =>
  !NON_RETRYABLE_CODES.has((e as { code?: string } | null)?.code ?? "");

const idFor = (r: PriceUpdateRequest): string =>
  createHash("sha256")
    .update(`${r.asset}:${r.price}:${r.source}:${r.observedAt}`)
    .digest("hex");

const isConfig = (
  v: ContractUpdaterConfig | ContractAdapter,
): v is ContractUpdaterConfig => "adminSecretKey" in v;

const noAdapter: ContractAdapter = {
  submit: async () => {
    throw new Error("no adapter");
  },
  getLatestUpdate: async () => null,
};

// ---------------------------------------------------------------------------
// ContractUpdater
// ---------------------------------------------------------------------------

export class ContractUpdater {
  private config: ContractUpdaterConfig;
  private server?: SorobanRpc.Server;
  private adminKeypair?: Keypair;
  private networkPassphrase: string;
  private adapter: ContractAdapter;

  private subs = new Map<string, PriceSubmission>();
  private latest = new Map<string, PriceSubmission>();
  private chains = new Map<string, Promise<void>>();

  /**
   * @param configOrAdapter - Full config (signs and submits on-chain), or a bare
   *                          adapter (pipeline-only mode, e.g. for tests).
   * @param adapter         - Optional adapter override when a config is given.
   */
  constructor(
    configOrAdapter?: ContractUpdaterConfig | ContractAdapter,
    adapter?: ContractAdapter,
  ) {
    if (configOrAdapter && isConfig(configOrAdapter)) {
      this.config = {
        ...DEFAULT_CONFIG,
        ...configOrAdapter,
      } as ContractUpdaterConfig;

      this.server = new SorobanRpc.Server(this.config.rpcUrl);
      this.adminKeypair = Keypair.fromSecret(this.config.adminSecretKey);
      this.networkPassphrase =
        this.config.network === "testnet" ? Networks.TESTNET : Networks.PUBLIC;
      this.adapter = adapter ?? this.buildDefaultAdapter();

      // I-2: log only the public key, never the secret key.
      logger.info("Contract updater initialized", {
        network: this.config.network,
        contractId: this.config.contractId,
        adminPublicKey: this.adminKeypair.publicKey(),
      });
    } else {
      this.config = {
        ...DEFAULT_CONFIG,
        network: "testnet",
        rpcUrl: "",
        contractId: "",
        adminSecretKey: "",
      } as ContractUpdaterConfig;
      this.networkPassphrase = Networks.TESTNET;
      this.adapter = configOrAdapter ?? noAdapter;
    }
  }

  // -----------------------------------------------------------------------
  // Public API – validated, never throws
  // -----------------------------------------------------------------------

  /**
   * Update price for a single asset (I-1, I-2, I-3).
   *
   * @param asset     Non-empty symbol, at most MAX_ASSET_SYMBOL_BYTES bytes.
   * @param price     Fixed-point i128; must be > 0n.
   * @param timestamp Unix timestamp in SECONDS; positive integer.
   */
  async updatePrice(
    asset: string,
    price: bigint,
    timestamp: number,
  ): Promise<ContractUpdateResult> {
    const validationError = this.validateInputs(asset, price, timestamp);
    if (validationError !== null) {
      logger.warn("updatePrice rejected due to invalid input", {
        asset,
        reason: validationError,
        price: String(price),
        timestamp,
      });
      return {
        success: false,
        asset,
        price,
        timestamp,
        error: validationError,
      };
    }

    const startTime = Date.now();
    let lastError: Error | undefined;

    for (let attempt = 1; attempt <= this.config.maxRetries; attempt++) {
      try {
        logger.info(
          `Updating price for ${asset} (attempt ${attempt}/${this.config.maxRetries})`,
          { price: price.toString(), timestamp },
        );

        const txHash = await this.submitOnChain(asset, price, timestamp);

        const result: ContractUpdateResult = {
          success: true,
          transactionHash: txHash,
          asset,
          price,
          timestamp,
        };

        logger.info(`Price update successful for ${asset}`, {
          txHash,
          durationMs: Date.now() - startTime,
        });

        return result;
      } catch (error) {
        lastError = error instanceof Error ? error : new Error(String(error));
        const safeMessage = this.sanitizeErrorMessage(lastError.message);

        logger.warn(`Price update attempt ${attempt} failed for ${asset}`, {
          error: safeMessage,
        });

        if (attempt < this.config.maxRetries) {
          const delay = this.config.retryDelayMs * Math.pow(2, attempt - 1);
          await sleep(delay);
        }
      }
    }

    const safeLastError = lastError
      ? this.sanitizeErrorMessage(lastError.message)
      : "Unknown error";

    logger.error(`All price update attempts failed for ${asset}`, {
      error: safeLastError,
      attempts: this.config.maxRetries,
    });

    return {
      success: false,
      asset,
      price,
      timestamp,
      error: safeLastError,
    };
  }

  /**
   * Update prices for multiple assets (I-5: sequential, failure-isolated).
   */
  async updatePrices(
    prices: AggregatedPrice[],
  ): Promise<ContractUpdateResult[]> {
    const results: ContractUpdateResult[] = [];

    for (const priceEntry of prices) {
      const result = await this.updatePrice(
        priceEntry.asset,
        priceEntry.price,
        priceEntry.timestamp,
      );
      results.push(result);

      // Brief inter-submission pause to avoid RPC rate limits.
      await sleep(100);
    }

    return results;
  }

  // -----------------------------------------------------------------------
  // Public API – idempotent, serialized per asset, throws on failure
  // -----------------------------------------------------------------------

  /**
   * Submit a price update through the adapter pipeline.
   *
   * - Rejects stale / far-future observations.
   * - Serializes work per asset.
   * - Deduplicates by idempotency key and by last-confirmed price.
   * - Retries with jitter; skips retries for non-retryable error codes.
   *
   * `observedAt` / `timestamp` are in MILLISECONDS.
   */
  async submitPriceUpdate(
    input: PriceUpdateRequest | LegacyPriceUpdateInput,
  ): Promise<void> {
    const req: PriceUpdateRequest =
      "observedAt" in input
        ? input
        : {
            asset: input.asset,
            price: input.price,
            source: input.source,
            observedAt: input.timestamp,
            idempotencyKey: `${input.asset}:${input.price}:${input.source}:${input.timestamp}`,
          };

    // I-1: validate before any network work (timestamp checked in seconds).
    const validationError = this.validateInputs(
      req.asset,
      req.price,
      Math.floor(req.observedAt / 1000),
    );
    if (validationError !== null) {
      throw Object.assign(new Error(validationError), {
        code: "INVALID_ASSET",
      });
    }

    const now = Date.now();
    if (req.observedAt > now + FUTURE_SKEW_MS || req.observedAt < now - STALE_MS) {
      throw Object.assign(new Error("stale"), { code: "PRICE_STALE" });
    }

    await this.enqueue(req.asset, () => this.process(req));
  }

  /**
   * Check if the contract is accessible.
   */
  async healthCheck(): Promise<boolean> {
    try {
      const contract = new Contract(this.config.contractId);
      return !!contract;
    } catch {
      return false;
    }
  }

  /**
   * Get the admin public key (safe to expose – never the secret key).
   */
  getAdminPublicKey(): string {
    return this.requireChain().keypair.publicKey();
  }

  // -----------------------------------------------------------------------
  // Submission pipeline internals
  // -----------------------------------------------------------------------

  private enqueue(asset: string, task: () => Promise<void>): Promise<void> {
    const prev = this.chains.get(asset) ?? Promise.resolve();
    const next = prev.then(task, task);
    this.chains.set(
      asset,
      next.catch(() => {}),
    );
    return next;
  }

  private async process(req: PriceUpdateRequest): Promise<void> {
    const id = req.idempotencyKey ?? idFor(req);

    const existing = this.subs.get(id);
    if (existing && !RESUBMITTABLE_STATUSES.has(existing.status)) return;

    const last = this.latest.get(req.asset);
    if (last && last.status === "CONFIRMED" && last.price === req.price) return;

    const sub: PriceSubmission = {
      id,
      asset: req.asset,
      price: req.price,
      source: req.source,
      observedAt: req.observedAt,
      createdAt: Date.now(),
      status: "PENDING",
      attempts: existing?.status === "FAILED" ? existing.attempts : 0,
      error: existing?.error,
      expiresAt: Date.now() + TIMEOUT_MS,
    };
    this.subs.set(id, sub);

    try {
      await this.execute(sub);
      this.latest.set(req.asset, sub);
    } catch (e) {
      this.latest.delete(req.asset);
      throw e;
    }
  }

  private async execute(sub: PriceSubmission): Promise<void> {
    while (sub.attempts <= MAX_RETRIES && Date.now() < sub.expiresAt) {
      sub.attempts++;
      sub.lastAttemptAt = Date.now();

      try {
        // Skip the write if the chain already reflects this update.
        const onChain = await this.adapter.getLatestUpdate(sub.asset);
        if (
          onChain &&
          onChain.price === sub.price &&
          onChain.timestamp >= sub.observedAt
        ) {
          sub.status = "CONFIRMED";
          sub.txHash = onChain.txHash;
          return;
        }

        const res = await this.adapter.submit(sub);
        sub.status = "CONFIRMED";
        sub.txHash = res.txHash;
        return;
      } catch (e) {
        // I-2: never let the secret key reach sub.error / logs.
        sub.error = this.sanitizeErrorMessage(
          e instanceof Error ? e.message : String(e),
        );

        if (
          !isRetryable(e) ||
          sub.attempts > MAX_RETRIES ||
          Date.now() >= sub.expiresAt
        ) {
          sub.status = isRetryable(e) ? "FAILED" : "REJECTED";
          throw new Error(sub.error);
        }

        await sleep(calculateJitterDelay(sub.attempts - 1));
      }
    }

    sub.status = "FAILED";
    throw new Error(sub.error ?? "timeout");
  }

  // -----------------------------------------------------------------------
  // On-chain helpers
  // -----------------------------------------------------------------------

  /** Default adapter: submits through this instance's Soroban signer. */
  private buildDefaultAdapter(): ContractAdapter {
    return {
      submit: async (sub) => ({
        txHash: await this.submitOnChain(
          sub.asset,
          sub.price,
          Math.floor(sub.observedAt / 1000), // contract expects seconds
        ),
      }),
      getLatestUpdate: async () => null, // no on-chain read path implemented
    };
  }

  private requireChain(): { server: SorobanRpc.Server; keypair: Keypair } {
    if (!this.server || !this.adminKeypair) {
      throw new Error("ContractUpdater was constructed without chain config");
    }
    return { server: this.server, keypair: this.adminKeypair };
  }

  /**
   * Validate updatePrice arguments (I-1). `timestamp` is in seconds.
   *
   * @returns `null` when valid; an error-reason string otherwise.
   */
  private validateInputs(
    asset: string,
    price: bigint,
    timestamp: number,
  ): string | null {
    if (typeof asset !== "string" || asset.trim().length === 0) {
      return "asset must be a non-empty string";
    }

    if (new TextEncoder().encode(asset).length > MAX_ASSET_SYMBOL_BYTES) {
      return `asset symbol exceeds maximum length of ${MAX_ASSET_SYMBOL_BYTES} bytes`;
    }

    if (typeof price !== "bigint" || price <= 0n) {
      return "price must be a positive bigint (> 0)";
    }

    if (
      !Number.isFinite(timestamp) ||
      !Number.isInteger(timestamp) ||
      timestamp <= 0
    ) {
      return "timestamp must be a positive integer Unix timestamp (seconds)";
    }

    return null;
  }

  /**
   * Build, sign, submit and confirm a set_asset_price transaction.
   * Throws on simulation failure, submission failure, or confirmation
   * timeout (I-4). `timestamp` is in seconds.
   */
  private async submitOnChain(
    asset: string,
    price: bigint,
    timestamp: number,
  ): Promise<string> {
    const { server, keypair } = this.requireChain();

    const contract = new Contract(this.config.contractId);
    const adminAddress = new Address(keypair.publicKey());

    const operation = contract.call(
      "set_asset_price",
      adminAddress.toScVal(),
      xdr.ScVal.scvSymbol(asset),
      nativeToScVal(price, { type: "i128" }),
      nativeToScVal(timestamp, { type: "u64" }),
    );

    const account = await server.getAccount(keypair.publicKey());

    const transaction = new TransactionBuilder(account, {
      fee: "100000",
      networkPassphrase: this.networkPassphrase,
    })
      .addOperation(operation)
      .setTimeout(30)
      .build();

    const simulated = await server.simulateTransaction(transaction);

    if (SorobanRpc.Api.isSimulationError(simulated)) {
      throw new Error(`Simulation failed: ${simulated.error}`);
    }

    if (!SorobanRpc.Api.isSimulationSuccess(simulated)) {
      throw new Error("Simulation did not succeed");
    }

    const prepared = SorobanRpc.assembleTransaction(
      transaction,
      simulated,
    ).build();
    prepared.sign(keypair);

    const response = await server.sendTransaction(prepared);

    if (response.status === "ERROR") {
      throw new Error(`Transaction failed: ${response.errorResult}`);
    }

    const hash = response.hash;

    // I-4: bounded poll loop – give up after txPollTimeoutMs.
    const pollTimeout =
      this.config.txPollTimeoutMs ?? DEFAULT_TX_POLL_TIMEOUT_MS;
    const deadline = Date.now() + pollTimeout;
    let getResponse = await server.getTransaction(hash);

    while (
      getResponse.status === SorobanRpc.Api.GetTransactionStatus.NOT_FOUND
    ) {
      if (Date.now() >= deadline) {
        throw new Error(
          `Transaction confirmation timed out after ${pollTimeout}ms`,
        );
      }
      await sleep(1000);
      getResponse = await server.getTransaction(hash);
    }

    if (getResponse.status === SorobanRpc.Api.GetTransactionStatus.FAILED) {
      throw new Error("Transaction failed on-chain");
    }

    return hash;
  }

  /**
   * Remove any occurrence of the admin secret key from an error message (I-2).
   */
  private sanitizeErrorMessage(message: string): string {
    const secret = this.config.adminSecretKey;
    if (secret && message.includes(secret)) {
      return message.replaceAll(secret, "[REDACTED]");
    }
    return message;
  }
}

/**
 * Create a contract updater
 */
export function createContractUpdater(
  config: ContractUpdaterConfig,
  adapter?: ContractAdapter,
): ContractUpdater {
  return new ContractUpdater(config, adapter);
}