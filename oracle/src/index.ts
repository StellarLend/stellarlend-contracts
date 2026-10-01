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
import { AdminServer } from './services/admin-server.js';
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
    private adminServer?: AdminServer;
    private isRunning: boolean = false;

    constructor(config: OracleServiceConfig) {
        if (config.stellarNetwork && !['testnet', 'mainnet'].includes(config.stellarNetwork)) {
            throw new Error(`Invalid stellar network: ${config.stellarNetwork}`);
        }
        if (config.stellarRpcUrl) {
            try {
                new URL(config.stellarRpcUrl);
            } catch {
                throw new Error(`Invalid RPC URL: ${config.stellarRpcUrl}`);
            }
        }
        if (config.contractId !== undefined && config.contractId.trim() === '') {
            throw new Error('Contract ID cannot be empty');
        }

        // Normalize providers if single provider
        let normalizedProviders = config.providers;
        if (normalizedProviders && normalizedProviders.length === 1) {
            normalizedProviders = [{ ...normalizedProviders[0], weight: 1.0 }];
        }

        this.config = {
            ...config,
            providers: normalizedProviders,
        };

        // Configure logging
        configureLogger(this.config.logLevel);

        // Create providers from configuration
        const providers: BasePriceProvider[] = (this.config.providers || [])
            .filter((p) => p.enabled)
            .map((p) => {
                switch (p.name) {
                    case 'coingecko':
                        return createCoinGeckoProvider(p as ProviderConfig);
                    case 'binance':
                        return createBinanceProvider(p as ProviderConfig);
                    default:
                        logger.warn('Unknown provider in config, skipping', { provider: p.name });
                        return null;
                }
            })
            .filter((x): x is BasePriceProvider => x !== null)
            .sort((a, b) => a.priority - b.priority);


        // Create services
        const validator = createValidator(
            {
                maxDeviationPercent: Math.max(1, this.config.maxPriceDeviationPercent ?? 10),
                maxStalenessSeconds: Math.max(1, this.config.priceStaleThresholdSeconds ?? 300),
            },
            this.config.priceBounds,
        );

        const cache = createPriceCache(Math.max(0, this.config.cacheTtlSeconds ?? 30));

        this.aggregator = createAggregator(providers, validator, cache);

        this.contractUpdater = createContractUpdater({
            network: this.config.stellarNetwork,
            rpcUrl: this.config.stellarRpcUrl,
            contractId: this.config.contractId,
            adminSecretKey: this.config.adminSecretKey,
            maxRetries: 3,
            retryDelayMs: 1000,
        });

        if (this.config.adminApiPort > 0) {
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
            providers: [...(this.config.providers || [])],
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

