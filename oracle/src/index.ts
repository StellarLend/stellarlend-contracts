/**
 * StellarLend Oracle Service
 * 
 * Off-chain oracle integration service that fetches price data from
 * multiple sources (CoinGecko, Binance)
 * @see https://github.com/stellarlend/stellarlend-contracts
 */

import { loadConfig, type OracleServiceConfig } from './config.js';
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
import type { ProviderConfig } from './types/index.js';

/**
 * Default assets to fetch prices for
 */
const DEFAULT_ASSETS = ['XLM', 'USDC', 'BTC', 'ETH', 'SOL'];

/**
 * Oracle Service
 */
export class OracleService {
    private config: OracleServiceConfig;
    private aggregator: PriceAggregator;
    private contractUpdater: ContractUpdater;
    private intervalId?: ReturnType<typeof setInterval>;
    private isRunning: boolean = false;

    constructor(config: OracleServiceConfig) {
        this.config = config;

        // Configure logging
        configureLogger(config.logLevel);

        // Create providers
        const providers: BasePriceProvider[] = [
            createCoinGeckoProvider(
                config.providers.find((p: ProviderConfig) => p.name === 'coingecko')?.apiKey,
            ),
            createBinanceProvider(),
        ];


        // Create services
        const validator = createValidator({
            maxDeviationPercent: config.maxPriceDeviationPercent,
            maxStalenessSeconds: config.priceStaleThresholdSeconds,
        });

        const cache = createPriceCache(config.cacheTtlSeconds);

        this.aggregator = createAggregator(providers, validator, cache);

        this.contractUpdater = createContractUpdater({
            network: config.stellarNetwork,
            rpcUrl: config.stellarRpcUrl,
            contractId: config.contractId,
            adminSecretKey: config.adminSecretKey,
            maxRetries: 3,
            retryDelayMs: 1000,
        });

        logger.info('Oracle service initialized', {
            network: config.stellarNetwork,
            contractId: config.contractId,
            updateInterval: config.updateIntervalMs,
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
    }

    /**
     * Stop the oracle service
     */
    stop(): void {
        if (!this.isRunning) {
            logger.warn('Oracle service is not running');
            return;
        }

        if (this.intervalId) {
            clearInterval(this.intervalId);
            this.intervalId = undefined;
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
            providers: this.aggregator.getProviders(),
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

// Run if this is the main module
main().catch(console.error);

// Export for programmatic use
export { loadConfig } from './config.js';
export type { OracleServiceConfig } from './config.js';

