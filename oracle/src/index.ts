/**
 * StellarLend Oracle Service
 *
 * Off-chain oracle integration service that fetches price data from
 * multiple sources (CoinGecko, Binance) and updates on-chain contracts.
 * @see https://github.com/stellarlend/stellarlend-contracts
 */

import { loadConfig, type OracleServiceConfig } from "./config.js";
import { configureLogger, logger } from "./utils/logger.js";
import {
  createCoinGeckoProvider,
  createBinanceProvider,
  type BasePriceProvider,
} from "./providers/index.js";
import {
  createValidator,
  createPriceCache,
  createAggregator,
  createContractUpdater,
  type PriceAggregator,
  type ContractUpdater,
  type PriceValidator,
  type PriceCache,
} from "./services/index.js";
import { AdminServer } from "./services/admin-server.js";
import type {
  ProviderConfig,
  ContractUpdateResult,
  AggregatedPrice,
} from "./types/index.js";

/**
 * Default assets to fetch prices for
 */
export const DEFAULT_ASSETS = ["XLM", "USDC", "BTC", "ETH", "USDT"] as const;

/**
 * Service status structure for monitoring and health diagnostics
 */
export interface OracleServiceStatus {
  isRunning: boolean;
  isUpdating: boolean;
  network: string;
  contractId: string;
  updateIntervalMs: number;
  providers: ProviderConfig[];
  aggregatorStats?: Record<string, unknown>;
  lastUpdateTimestamp?: number;
  lastUpdateDurationMs?: number;
  lastUpdateResults?: {
    total: number;
    successful: number;
    failed: number;
  };
}

/**
 * Optional dependencies for dependency injection and controlled testing
 */
export interface OracleServiceDependencies {
  aggregator?: PriceAggregator;
  contractUpdater?: ContractUpdater;
  adminServer?: AdminServer;
  validator?: PriceValidator;
  cache?: PriceCache;
}

/**
 * Sanitizes sensitive fields to ensure logs never expose secrets.
 */
export function sanitizeLogConfig(
  config: Partial<OracleServiceConfig>,
): Record<string, unknown> {
  const sanitized: Record<string, unknown> = {};
  for (const [key, value] of Object.entries(config)) {
    if (/secret|key|password|token|hmac/i.test(key)) {
      sanitized[key] = "[REDACTED]";
    } else {
      sanitized[key] = value;
    }
  }
  return sanitized;
}

/**
 * Validates configuration boundaries and required parameters.
 */
export function validateOracleConfig(config: OracleServiceConfig): void {
  if (!config || typeof config !== "object") {
    throw new Error("Oracle configuration must be a non-null object");
  }

  if (
    config.stellarNetwork !== "testnet" &&
    config.stellarNetwork !== "mainnet"
  ) {
    throw new Error(
      `Invalid stellarNetwork: '${config.stellarNetwork}'. Must be 'testnet' or 'mainnet'`,
    );
  }

  if (
    !config.stellarRpcUrl ||
    typeof config.stellarRpcUrl !== "string" ||
    !/^https?:\/\//.test(config.stellarRpcUrl)
  ) {
    throw new Error("stellarRpcUrl must be a valid HTTP/HTTPS URL");
  }

  if (
    config.contractId !== undefined &&
    (typeof config.contractId !== "string" ||
      config.contractId.trim().length === 0)
  ) {
    throw new Error("contractId is required and must be a non-empty string");
  }

  if (config.adminApiPort !== undefined && config.adminApiPort > 0) {
    if (!config.adminHmacSecret || config.adminHmacSecret.trim().length === 0) {
      throw new Error(
        "ADMIN_HMAC_SECRET is required when ADMIN_API_PORT is configured",
      );
    }
  }
}

/**
 * Read-only view of a configured provider, reported by `getStatus()`.
 * Derived from the validated configuration so that status always reflects what
 * was configured, independently of provider runtime state.
 */
export interface ProviderStatus {
    name: string;
    enabled: boolean;
    priority: number;
    weight: number;
}

/**
 * Oracle Service
 */
export class OracleService {
  private readonly config: OracleServiceConfig;
  private readonly configuredProviders: ProviderConfig[];
  private readonly aggregator: PriceAggregator;
  private readonly contractUpdater: ContractUpdater;
  private readonly adminServer?: AdminServer;
  private intervalId?: ReturnType<typeof setInterval>;
  private isRunning: boolean = false;
  private isUpdating: boolean = false;
  private lastUpdateTimestamp?: number;
  private lastUpdateDurationMs?: number;
  private lastUpdateResults?: {
    total: number;
    successful: number;
    failed: number;
  };

  constructor(
    config: OracleServiceConfig,
    dependencies: OracleServiceDependencies = {},
  ) {
    validateOracleConfig(config);
    this.config = config;

    // Normalize configured providers (if single provider, normalize its weight to 1.0)
    this.configuredProviders = (config.providers || []).map((p) => ({ ...p }));
    if (this.configuredProviders.length === 1) {
      this.configuredProviders[0].weight = 1.0;
    }

    // Configure logging
    configureLogger(config.logLevel);

    // Normalize boundaries gracefully
    const safeDeviation =
      typeof config.maxPriceDeviationPercent === "number" &&
      config.maxPriceDeviationPercent > 0
        ? config.maxPriceDeviationPercent
        : 10;
    const safeStaleness =
      typeof config.priceStaleThresholdSeconds === "number" &&
      config.priceStaleThresholdSeconds > 0
        ? config.priceStaleThresholdSeconds
        : 300;
    const safeCacheTtl =
      typeof config.cacheTtlSeconds === "number" && config.cacheTtlSeconds > 0
        ? config.cacheTtlSeconds
        : 30;

    // Instantiate validator
    const validator =
      dependencies.validator ??
      createValidator(
        {
          maxDeviationPercent: safeDeviation,
          maxStalenessSeconds: safeStaleness,
        },
        config.priceBounds,
      );

    // Instantiate cache
    const cache = dependencies.cache ?? createPriceCache(safeCacheTtl);

    // Instantiate providers from configuration
    const providers: BasePriceProvider[] = (config.providers || [])
      .filter((p) => p && p.enabled)
      .map((p) => {
        switch (p.name?.toLowerCase()) {
          case "coingecko":
            return createCoinGeckoProvider(p.apiKey);
          case "binance":
            return createBinanceProvider();
          default:
            logger.warn("Unknown provider in config, skipping", {
              provider: p.name,
            });
            return null;
        }
      })
      .filter((x): x is BasePriceProvider => x !== null)
      .sort((a, b) => (a.priority ?? 0) - (b.priority ?? 0));

    // Instantiate aggregator
    this.aggregator =
      dependencies.aggregator ?? createAggregator(providers, validator, cache);

    // Instantiate contract updater
    this.contractUpdater =
      dependencies.contractUpdater ??
      createContractUpdater({
        network: config.stellarNetwork,
        rpcUrl: config.stellarRpcUrl,
        contractId: config.contractId,
        adminSecretKey: config.adminSecretKey,
        maxRetries: 3,
        retryDelayMs: 1000,
      });

    // Instantiate optional admin server
    if (dependencies.adminServer) {
      this.adminServer = dependencies.adminServer;
    } else if (config.adminApiPort && config.adminApiPort > 0) {
      this.adminServer = new AdminServer({
        port: config.adminApiPort,
        hmacSecret: config.adminHmacSecret!,
        validator,
      });
    }

    logger.info("Oracle service initialized", {
      network: config.stellarNetwork,
      contractId: config.contractId,
      updateInterval: config.updateIntervalMs,
      providers: (config.providers || []).map((p) => p.name),
    });
  }

  /**
   * Start the oracle service
   */
  async start(assets: string[] = [...DEFAULT_ASSETS]): Promise<void> {
    if (this.isRunning) {
      logger.warn("Oracle service is already running");
      return;
    }

    this.isRunning = true;
    logger.info("Starting oracle service", { assets });

    // Run immediately on start (catching failure to ensure startup determinism)
    try {
      await this.updatePrices(assets);
    } catch (error) {
      logger.error("Initial price update cycle failed during service startup", {
        error: error instanceof Error ? error.message : String(error),
      });
    }

    const updateInterval =
      typeof this.config.updateIntervalMs === "number" &&
      this.config.updateIntervalMs > 0
        ? this.config.updateIntervalMs
        : 60000;

    // Schedule periodic updates
    this.intervalId = setInterval(async () => {
      try {
        await this.updatePrices(assets);
      } catch (error) {
        logger.error("Scheduled price update cycle failed", {
          error: error instanceof Error ? error.message : String(error),
        });
      }
    }, updateInterval);

    logger.info("Oracle service started", {
      intervalMs: updateInterval,
    });

    if (this.adminServer) {
      try {
        await this.adminServer.start();
      } catch (error) {
        logger.error("Failed to start admin server during service startup", {
          error: error instanceof Error ? error.message : String(error),
        });
        await this.stop();
        throw error;
      }
    }
  }

  /**
   * Stop the oracle service
   */
  async stop(): Promise<void> {
    if (!this.isRunning) {
      logger.warn("Oracle service is not running");
      return;
    }

    if (this.intervalId) {
      clearInterval(this.intervalId);
      this.intervalId = undefined;
    }

    if (this.adminServer) {
      try {
        await this.adminServer.stop();
      } catch (error) {
        logger.error("Error stopping admin server", {
          error: error instanceof Error ? error.message : String(error),
        });
      }
    }

    this.isRunning = false;
    logger.info("Oracle service stopped");
  }

  /**
   * Fetch and update prices for specified assets with strict input validation,
   * concurrency protection, and partial failure handling.
   */
  async updatePrices(assets: string[]): Promise<ContractUpdateResult[]> {
    if (!Array.isArray(assets)) {
      throw new TypeError("Assets must be an array of asset symbols");
    }

    const sanitizedAssets = Array.from(
      new Set(
        assets
          .filter((a): a is string => typeof a === "string")
          .map((a) => a.trim().toUpperCase())
          .filter((a) => a.length > 0),
      ),
    );

    if (sanitizedAssets.length === 0) {
      logger.warn("No valid assets provided for price update cycle");
      return [];
    }

    // Concurrency guard: prevent overlapping update cycles on the same contract
    if (this.isUpdating) {
      logger.warn(
        "Price update cycle already in progress, skipping overlapping execution",
      );
      return [];
    }

    this.isUpdating = true;
    const startTime = Date.now();
    logger.info("Starting price update cycle", { assets: sanitizedAssets });

    try {
      // Fetch aggregated prices
      const prices = await this.aggregator.getPrices(sanitizedAssets);

      if (!prices || prices.size === 0) {
        logger.error("No prices fetched from any provider", {
          requestedAssets: sanitizedAssets,
        });
        this.lastUpdateTimestamp = Date.now();
        this.lastUpdateDurationMs = Date.now() - startTime;
        this.lastUpdateResults = {
          total: 0,
          successful: 0,
          failed: sanitizedAssets.length,
        };
        return [];
      }

      // Diagnostic reporting for partial fetch failures
      if (prices.size < sanitizedAssets.length) {
        const missingAssets = sanitizedAssets.filter((a) => !prices.has(a));
        logger.warn("Partial price fetch: some assets could not be retrieved", {
          missingAssets,
          retrievedCount: prices.size,
          totalRequested: sanitizedAssets.length,
        });
      }

      logger.info(`Fetched ${prices.size} prices`, {
        assets: Array.from(prices.keys()),
      });

      // Submit updates to contract updater with interface compatibility
      const priceArray = Array.from(prices.values());
      let results: ContractUpdateResult[] = [];

      if (typeof (this.contractUpdater as any).updatePrices === "function") {
        results = await (this.contractUpdater as any).updatePrices(priceArray);
      } else if (
        typeof (this.contractUpdater as any).submitPriceUpdate === "function"
      ) {
        results = await Promise.all(
          priceArray.map(async (p): Promise<ContractUpdateResult> => {
            try {
              await (this.contractUpdater as any).submitPriceUpdate(p);
              return {
                success: true,
                asset: p.asset,
                price: p.price,
                timestamp: p.timestamp,
              };
            } catch (err) {
              return {
                success: false,
                asset: p.asset,
                price: p.price,
                timestamp: p.timestamp,
                error: err instanceof Error ? err.message : String(err),
              };
            }
          }),
        );
      } else {
        throw new Error(
          "ContractUpdater does not implement updatePrices or submitPriceUpdate",
        );
      }

      // Log results
      const successful = results.filter((r) => r.success);
      const failed = results.filter((r) => !r.success);

      this.lastUpdateTimestamp = Date.now();
      this.lastUpdateDurationMs = Date.now() - startTime;
      this.lastUpdateResults = {
        total: results.length,
        successful: successful.length,
        failed: failed.length,
      };

      logger.info("Price update cycle complete", {
        successful: successful.length,
        failed: failed.length,
        durationMs: this.lastUpdateDurationMs,
      });

      if (failed.length > 0) {
        logger.warn("Some price updates failed", {
          failedAssets: failed.map((f) => f.asset),
        });
      }

      return results;
    } catch (error) {
      logger.error("Price update cycle failed", {
        error: error instanceof Error ? error.message : String(error),
      });
      this.lastUpdateTimestamp = Date.now();
      this.lastUpdateDurationMs = Date.now() - startTime;
      return [];
    } finally {
      this.isUpdating = false;
    }
  }

  /**
   * Get current service status
   */
  getStatus(): OracleServiceStatus {
    return {
      isRunning: this.isRunning,
      isUpdating: this.isUpdating,
      network: this.config.stellarNetwork,
      contractId: this.config.contractId,
      updateIntervalMs:
        typeof this.config.updateIntervalMs === "number" &&
        this.config.updateIntervalMs > 0
          ? this.config.updateIntervalMs
          : 60000,
      providers: this.configuredProviders,
      aggregatorStats:
        typeof this.aggregator.getStats === "function"
          ? this.aggregator.getStats()
          : undefined,
      lastUpdateTimestamp: this.lastUpdateTimestamp,
      lastUpdateDurationMs: this.lastUpdateDurationMs,
      lastUpdateResults: this.lastUpdateResults,
    };
  }

  /**
   * Manually fetch price for a single asset with validation
   */
  async fetchPrice(asset: string): Promise<AggregatedPrice | null> {
    if (typeof asset !== "string" || asset.trim().length === 0) {
      throw new TypeError("Asset symbol must be a non-empty string");
    }
    return this.aggregator.getPrice(asset.trim().toUpperCase());
  }
}

/**
 * Factory function to create OracleService
 */
export function createOracleService(
  config: OracleServiceConfig,
  dependencies?: OracleServiceDependencies,
): OracleService {
  return new OracleService(config, dependencies);
}

/**
 * Main entry point
 */
export async function main(): Promise<void> {
  console.log(`
╔═══════════════════════════════════════════════════════════╗
║                StellarLend Oracle Service                  ║
║                                                            ║
║  Off-chain oracle integration for price data management   ║
╚═══════════════════════════════════════════════════════════╝
  `);

  try {
    // Load configuration
    const config = loadConfig();

    // Create and start service
    const service = new OracleService(config);

    // Handle shutdown
    process.on("SIGINT", () => {
      logger.info("Received SIGINT, shutting down...");
      service.stop();
      process.exit(0);
    });

    process.on("SIGTERM", () => {
      logger.info("Received SIGTERM, shutting down...");
      service.stop();
      process.exit(0);
    });

    // Start service
    await service.start();
  } catch (error) {
    console.error("Failed to start oracle service:", error);
    process.exit(1);
  }
}

// Run if this is the main module
if (
  process.env.NODE_ENV !== "test" &&
  import.meta.url === `file://${process.argv[1]}`
) {
  main().catch(console.error);
}

// Export for programmatic use
export { loadConfig } from "./config.js";
export type { OracleServiceConfig } from "./config.js";
