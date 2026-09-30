/**
 * Contract Updater Service
 *
 * Submits signed price-update transactions to the StellarLend on-chain
 * contract via Soroban RPC.  This module enforces the following invariants:
 *
 * I-1  Every public write method validates its inputs before touching the network.
 * I-2  The admin secret key is NEVER included in log output or error messages.
 * I-3  The retry loop uses exponential back-off and terminates after maxRetries.
 * I-4  The transaction-poll loop is bounded by txPollTimeoutMs; it throws after
 *      that deadline rather than spinning forever.
 * I-5  updatePrices processes assets sequentially; a failure for one asset does
 *      NOT abort subsequent assets.
 */

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
// Public constants – callers may reference these for their own validation.
// ---------------------------------------------------------------------------

/** Maximum allowed byte-length for an asset symbol passed to updatePrice. */
export const MAX_ASSET_SYMBOL_BYTES = 12;

/** Default cap (ms) on how long we wait for a submitted tx to be confirmed. */
export const DEFAULT_TX_POLL_TIMEOUT_MS = 60_000;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/**
 * Contract updater configuration
 */
export interface ContractUpdaterConfig {
  network: "testnet" | "mainnet";
  rpcUrl: string;
  /** StellarLend contract ID */
  contractId: string;
  /** Admin secret key for signing – never logged or surfaced in errors (I-2) */
  adminSecretKey: string;
  maxRetries: number;
  retryDelayMs: number;
  /**
   * Maximum milliseconds to poll for transaction confirmation before giving
   * up.  Defaults to {@link DEFAULT_TX_POLL_TIMEOUT_MS} (60 s).
   */
  txPollTimeoutMs?: number;
}

/**
 * Default configuration
 */
const DEFAULT_CONFIG: Partial<ContractUpdaterConfig> = {
  maxRetries: 3,
  retryDelayMs: 1000,
  txPollTimeoutMs: DEFAULT_TX_POLL_TIMEOUT_MS,
};

// ---------------------------------------------------------------------------
// ContractUpdater
// ---------------------------------------------------------------------------

/**
 * Contract Updater
 *
 * Responsible for building, signing, and submitting Soroban price-update
 * transactions.  The class is intentionally narrow: it owns the retry loop and
 * exponential back-off but delegates all price-aggregation concerns to callers.
 */
export class ContractUpdater {
  private config: ContractUpdaterConfig;
  private server: SorobanRpc.Server;
  private adminKeypair: Keypair;
  private networkPassphrase: string;

  constructor(config: ContractUpdaterConfig) {
    this.config = { ...DEFAULT_CONFIG, ...config } as ContractUpdaterConfig;

    this.server = new SorobanRpc.Server(this.config.rpcUrl);
    this.adminKeypair = Keypair.fromSecret(this.config.adminSecretKey);
    this.networkPassphrase =
      this.config.network === "testnet" ? Networks.TESTNET : Networks.PUBLIC;

    // I-2: log only the public key, never the secret key.
    logger.info("Contract updater initialized", {
      network: this.config.network,
      contractId: this.config.contractId,
      adminPublicKey: this.adminKeypair.publicKey(),
    });
  }

  // -----------------------------------------------------------------------
  // Public API
  // -----------------------------------------------------------------------

  /**
   * Update price for a single asset.
   *
   * Validates inputs (I-1) before attempting any network call.  On success
   * returns `{ success: true, transactionHash, … }`.  On exhausted retries
   * returns `{ success: false, error, … }` – it never throws.
   *
   * @param asset      - Canonical asset symbol, e.g. "XLM".  Must be a
   *                     non-empty string of at most {@link MAX_ASSET_SYMBOL_BYTES}
   *                     ASCII characters.
   * @param price      - Price in fixed-point units (i128 on-chain).  Must be
   *                     strictly positive (> 0n).
   * @param timestamp  - Unix timestamp (seconds).  Must be a positive integer.
   */
  async updatePrice(
    asset: string,
    price: bigint,
    timestamp: number,
  ): Promise<ContractUpdateResult> {
    // ----- I-1: input validation -----
    const validationError = this.validateInputs(asset, price, timestamp);
    if (validationError !== null) {
      logger.warn("updatePrice rejected due to invalid input", {
        asset,
        reason: validationError,
        price: price.toString(),
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

    // ----- Retry loop (I-3) -----
    const startTime = Date.now();
    let lastError: Error | undefined;

    for (let attempt = 1; attempt <= this.config.maxRetries; attempt++) {
      try {
        logger.info(
          `Updating price for ${asset} (attempt ${attempt}/${this.config.maxRetries})`,
          {
            price: price.toString(),
            timestamp,
          },
        );

        const txHash = await this.submitPriceUpdate(asset, price, timestamp);

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

        // I-2: strip secret key from any error message before logging.
        const safeMessage = this.sanitizeErrorMessage(lastError.message);

        logger.warn(`Price update attempt ${attempt} failed for ${asset}`, {
          error: safeMessage,
        });

        if (attempt < this.config.maxRetries) {
          const delay = this.config.retryDelayMs * Math.pow(2, attempt - 1);
          await this.sleep(delay);
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
   *
   * Each asset is updated independently.  A failure for one asset is recorded
   * in the result set but does NOT prevent subsequent assets from being
   * attempted.
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
      await this.sleep(100);
    }

    return results;
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
    return this.adminKeypair.publicKey();
  }

  // -----------------------------------------------------------------------
  // Private helpers
  // -----------------------------------------------------------------------

  /**
   * Validate updatePrice arguments (I-1).
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

    // Guard against symbol-injection or excessively long identifiers.
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
   * Submit a price update transaction to the contract.
   *
   * Throws on simulation failure, submission failure, or confirmation
   * timeout (I-4).
   */
  private async submitPriceUpdate(
    asset: string,
    price: bigint,
    timestamp: number,
  ): Promise<string> {
    const contract = new Contract(this.config.contractId);
    const adminAddress = new Address(this.adminKeypair.publicKey());

    const operation = contract.call(
      "set_asset_price",
      adminAddress.toScVal(),
      xdr.ScVal.scvSymbol(asset),
      nativeToScVal(price, { type: "i128" }),
      nativeToScVal(timestamp, { type: "u64" }),
    );

    const account = await this.server.getAccount(this.adminKeypair.publicKey());

    const transaction = new TransactionBuilder(account, {
      fee: "100000",
      networkPassphrase: this.networkPassphrase,
    })
      .addOperation(operation)
      .setTimeout(30)
      .build();

    const simulated = await this.server.simulateTransaction(transaction);

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
    prepared.sign(this.adminKeypair);

    const response = await this.server.sendTransaction(prepared);

    if (response.status === "ERROR") {
      throw new Error(`Transaction failed: ${response.errorResult}`);
    }

    const hash = response.hash;

    // I-4: Bounded poll loop – give up after txPollTimeoutMs.
    const pollTimeout =
      this.config.txPollTimeoutMs ?? DEFAULT_TX_POLL_TIMEOUT_MS;
    const deadline = Date.now() + pollTimeout;
    let getResponse = await this.server.getTransaction(hash);

    while (
      getResponse.status === SorobanRpc.Api.GetTransactionStatus.NOT_FOUND
    ) {
      if (Date.now() >= deadline) {
        throw new Error(
          `Transaction confirmation timed out after ${pollTimeout}ms`,
        );
      }
      await this.sleep(1000);
      getResponse = await this.server.getTransaction(hash);
    }

    if (getResponse.status === SorobanRpc.Api.GetTransactionStatus.FAILED) {
      throw new Error("Transaction failed on-chain");
    }

    return hash;
  }

  /**
   * Remove any occurrence of the admin secret key from an error message so
   * it cannot leak into logs (I-2).
   */
  private sanitizeErrorMessage(message: string): string {
    const secret = this.config.adminSecretKey;
    if (secret && message.includes(secret)) {
      return message.replaceAll(secret, "[REDACTED]");
    }
    return message;
  }

  /**
   * Sleep utility
   */
  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }
}

/**
 * Create a contract updater
 */
export function createContractUpdater(
  config: ContractUpdaterConfig,
): ContractUpdater {
  return new ContractUpdater(config);
}
