/**
 * StellarLend Oracle Service
 * 
 * Off-chain oracle integration service that fetches price data from
 * multiple sources (CoinGecko, Binance)
 * @see https://github.com/stellarlend/stellarlend-contracts
 */

import { pathToFileURL } from 'node:url';
import { loadConfig, validateOracleServiceConfig, MAD_Z_SCORE_THRESHOLD, type OracleServiceConfig } from './config.js';
import { configureLogger, logger } from './utils/logger.js';
import {
    createCoinGeckoProvider,
    createBinanceProvider,
    type BasePriceProvider,
} from './providers/index.js';
import {
    createValidator,
    createPriceCache,
    createAggregator,
    createContractUpdater,
    type PriceAggregator,
    type ContractUpdater,
} from './services/index.js';
import { AdminServer } from './services/admin-server.js';

/**
 * Default assets to fetch prices for
 */
const DEFAULT_ASSETS = ['XLM', 'USDC', 'BTC', 'ETH', 'SOL'];

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
    private config: OracleServiceConfig;
    private providerStatuses: ProviderStatus[];
    private aggregator: PriceAggregator;
    private contractUpdater: ContractUpdater;
    private intervalId?: ReturnType<typeof setInterval>;
    private adminServer?: AdminServer;
    private isRunning: boolean = false;

    constructor(config: OracleServiceConfig) {
        // Reject invalid configuration before any side effects are observable.
        this.config = validateOracleServiceConfig(config);

        // Configure logging
        configureLogger(this.config.logLevel);

        this.providerStatuses = this.config.providers
            .map((p) => ({
                name: p.name,
                enabled: p.enabled,
                priority: p.priority,
                weight: p.weight,
            }))
            .sort((a, b) => a.priority - b.priority);

        // Create providers from configuration. Names are constrained to the
        // supported set by validation, so a config entry can never be dropped.
        const providers: BasePriceProvider[] = this.config.providers
            .filter((p) => p.enabled)
            .map((p) => {
                switch (p.name) {
                    case 'coingecko':
                        return createCoinGeckoProvider(p.apiKey);
                    case 'binance':
                        return createBinanceProvider();
                    default:
                        // Unreachable: validateOracleServiceConfig rejects unknown names.
                        throw new Error(`Unsupported provider "${p.name}" in configuration`);
                }
            })
            .sort((a, b) => a.priority - b.priority);

        // Create services
        const validator = createValidator(
            {
                maxDeviationPercent: this.config.maxPriceDeviationPercent,
                maxStalenessSeconds: this.config.priceStaleThresholdSeconds,
            },
            this.config.priceBounds,
        );

        const cache = createPriceCache(this.config.cacheTtlSeconds);

        this.aggregator = createAggregator(providers, validator, cache, {
            madZScoreThreshold: this.config.madZScoreThreshold ?? MAD_Z_SCORE_THRESHOLD,
        });

        this.contractUpdater = createContractUpdater({
            network: this.config.stellarNetwork,
            rpcUrl: this.config.stellarRpcUrl,
            contractId: this.config.contractId,
            adminSecretKey: this.config.adminSecretKey,
            maxRetries: 3,
            retryDelayMs: 1000,
        });

        if (this.config.adminApiPort !== undefined && this.config.adminApiPort > 0) {
            if (!this.config.adminHmacSecret) {
                throw new Error('ADMIN_HMAC_SECRET is required when ADMIN_API_PORT is configured');
            }

            this.adminServer = new AdminServer({
                port: this.config.adminApiPort,
                hmacSecret: this.config.adminHmacSecret,
                validator,
            });
        }

        logger.info('Oracle service initialized', {
            network: this.config.stellarNetwork,
            contractId: this.config.contractId,
            updateInterval: this.config.updateIntervalMs,
            providers: this.aggregator.getProviders(),
        });
    }

    /**
     * Start the oracle service
     */
    async start(assets: string[] = DEFAULT_ASSETS): Promise<void> {
        if (this.isRunning) {
            logger.warn('Oracle service is already running');
            return;
        }

        this.isRunning = true;
        logger.info('Starting oracle service', { assets });

        // Run immediately on start
        await this.updatePrices(assets);

        // Schedule periodic updates
        this.intervalId = setInterval(async () => {
            await this.updatePrices(assets);
        }, this.config.updateIntervalMs);

        logger.info('Oracle service started', {
            intervalMs: this.config.updateIntervalMs,
        });

        if (this.adminServer) {
            await this.adminServer.start();
        }
    }

    /**
     * Stop the oracle service
     */
    async stop(): Promise<void> {
        if (!this.isRunning) {
            logger.warn('Oracle service is not running');
            return;
        }

        if (this.intervalId) {
            clearInterval(this.intervalId);
            this.intervalId = undefined;
        }

        if (this.adminServer) {
            await this.adminServer.stop();
        }

        this.isRunning = false;
        logger.info('Oracle service stopped');
    }

    /**
     * Fetch and update prices for specified assets
     */
    async updatePrices(assets: string[]): Promise<void> {
        const startTime = Date.now();

        logger.info('Starting price update cycle', { assets });

        try {
            // Fetch aggregated prices
            const prices = await this.aggregator.getPrices(assets);

            if (prices.size === 0) {
                logger.error('No prices fetched from any provider');
                return;
            }

            logger.info(`Fetched ${prices.size} prices`, {
                assets: Array.from(prices.keys()),
            });

            // Update contract
            const priceArray = Array.from(prices.values());
            const results = await this.contractUpdater.updatePrices(priceArray);

            // Log results
            const successful = results.filter((r) => r.success);
            const failed = results.filter((r) => !r.success);

            logger.info('Price update cycle complete', {
                successful: successful.length,
                failed: failed.length,
                durationMs: Date.now() - startTime,
            });

            if (failed.length > 0) {
                logger.warn('Some price updates failed', {
                    failedAssets: failed.map((f) => f.asset),
                });
            }
        } catch (error) {
            logger.error('Price update cycle failed', { error });
        }
    }

    /**
     * Get current service status
     */
    getStatus() {
        return {
            isRunning: this.isRunning,
            network: this.config.stellarNetwork,
            contractId: this.config.contractId,
            // Fresh copies: callers must not be able to mutate service state.
            providers: this.providerStatuses.map((p) => ({ ...p })),
            aggregatorStats: this.aggregator.getStats(),
        };
    }

    /**
     * Manually fetch price for a single asset (for testing)
     */
    async fetchPrice(asset: string) {
        return this.aggregator.getPrice(asset);
    }
}

/**
 * Main entry point
 */
async function main(): Promise<void> {
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
        process.on('SIGINT', () => {
            logger.info('Received SIGINT, shutting down...');
            service.stop();
            process.exit(0);
        });

        process.on('SIGTERM', () => {
            logger.info('Received SIGTERM, shutting down...');
            service.stop();
            process.exit(0);
        });

        // Start service
        await service.start();

    } catch (error) {
        console.error('Failed to start oracle service:', error);
        process.exit(1);
    }
}

/**
 * True when this module was executed directly (`node dist/index.js`) rather
 * than imported. Importing the module must not boot a service or terminate the
 * host process.
 */
function isDirectExecution(): boolean {
    const entry = process.argv[1];
    if (entry === undefined) {
        return false;
    }
    try {
        return import.meta.url === pathToFileURL(entry).href;
    } catch {
        return false;
    }
}

// Run only when this module is the process entry point.
if (isDirectExecution()) {
    main().catch(console.error);
}

// Export for programmatic use
export { loadConfig } from './config.js';
export type { OracleServiceConfig } from './config.js';

