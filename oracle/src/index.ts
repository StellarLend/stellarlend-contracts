/**
 * StellarLend Oracle Service
 *
 * Off-chain oracle integration service that fetches price data from
 * multiple sources (CoinGecko, Binance) and pushes signed price updates
 * to the Soroban lending contract.
 *
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
    OracleTelemetry,
    type PriceAggregator,
    type ContractUpdater,
    type TelemetrySummary,
} from './services/index.js';
import { AdminServer } from './services/admin-server.js';
import type { ProviderConfig, UpdateCycleDiagnostics, AggregatedPrice } from './types/index.js';

const DEFAULT_ASSETS = ['XLM', 'USDC', 'BTC', 'ETH'];

// ─── OracleService ────────────────────────────────────────────────────────────

export class OracleService {
    private config: OracleServiceConfig;
    private aggregator: PriceAggregator;
    private contractUpdater: ContractUpdater;
    private telemetry: OracleTelemetry;
    private intervalId?: ReturnType<typeof setInterval>;
    private adminServer?: AdminServer;
    private isRunning = false;

    constructor(config: OracleServiceConfig) {
        this.config = config;
        this.telemetry = new OracleTelemetry();

        configureLogger(config.logLevel);

        // Build providers from config, using the factory functions for each
        // known provider name. Unknown provider names are skipped with a warning.
        const providers: BasePriceProvider[] = config.providers
            .filter((p) => p.enabled)
            .flatMap((p: ProviderConfig): BasePriceProvider[] => {
                switch (p.name) {
                    case 'coingecko':
                        return [createCoinGeckoProvider(p.apiKey)];
                    case 'binance':
                        return [createBinanceProvider()];
                    default:
                        logger.warn('Unknown provider in config, skipping', { provider: p.name });
                        return [];
                }
            });

        const validator = createValidator(
            {
                maxDeviationPercent: config.maxPriceDeviationPercent,
                maxStalenessSeconds: config.priceStaleThresholdSeconds,
            },
            config.priceBounds ?? {},
        );

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

        if ((config.adminApiPort ?? 0) > 0) {
            if (!config.adminHmacSecret) {
                throw new Error('ADMIN_HMAC_SECRET is required when ADMIN_API_PORT is configured');
            }
            this.adminServer = new AdminServer({
                port: config.adminApiPort!,
                hmacSecret: config.adminHmacSecret,
                validator,
            });
        }

        logger.info('Oracle service initialized', {
            network: config.stellarNetwork,
            contractId: config.contractId,
            updateInterval: config.updateIntervalMs,
            providers: this.aggregator.getProviders(),
        });
    }

    // ─── Lifecycle ─────────────────────────────────────────────────────────────

    async start(assets: string[] = DEFAULT_ASSETS): Promise<void> {
        if (this.isRunning) {
            logger.warn('Oracle service is already running');
            return;
        }

        this.isRunning = true;
        logger.info('Starting oracle service', { assets });

        await this.updatePrices(assets);

        this.intervalId = setInterval(async () => {
            await this.updatePrices(assets);
        }, this.config.updateIntervalMs);

        logger.info('Oracle service started', { intervalMs: this.config.updateIntervalMs });

        if (this.adminServer) {
            await this.adminServer.start();
        }
    }

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

    // ─── Update cycle ──────────────────────────────────────────────────────────

    /**
     * Fetch prices for `assets`, push to contract, record structured diagnostics.
     * Never throws — errors are caught so the interval timer keeps running.
     */
    async updatePrices(assets: string[]): Promise<void> {
        const startedAt = new Date().toISOString();
        const cycleStart = Date.now();

        logger.info('oracle.cycle.start', { assets });

        const allProviderEvents: UpdateCycleDiagnostics['providerEvents'] = [];
        const cooledDownProviders = new Set<string>();
        let assetsUpdated = 0;
        let assetsFailed = 0;
        let contractUpdateOk = true;

        try {
            const prices = new Map<string, AggregatedPrice>();

            await Promise.allSettled(
                assets.map(async (asset) => {
                    const upper = asset.toUpperCase();

                    // Cache-hit path: getPrice returns a stub with sources=[] when cached.
                    const cached = await this.aggregator.getPrice(upper);
                    if (cached && cached.sources.length === 0) {
                        prices.set(upper, cached);
                        assetsUpdated += 1;
                        return;
                    }

                    // Cache miss: fetch with telemetry, then aggregate-and-cache in one pass.
                    const { prices: validPrices, events } =
                        await this.aggregator.fetchWithFallback(upper);

                    for (const ev of events) {
                        allProviderEvents.push(ev);
                        if (ev.errorClass === 'rate_limit' && ev.latencyMs === 0) {
                            cooledDownProviders.add(ev.provider);
                        }
                    }

                    if (validPrices.length === 0) {
                        assetsFailed += 1;
                        return;
                    }

                    // Single-pass: aggregate + write cache — no second round-trip.
                    const result = this.aggregator.aggregateAndCache(upper, validPrices);
                    if (result) {
                        prices.set(upper, result);
                        assetsUpdated += 1;
                    } else {
                        assetsFailed += 1;
                    }
                }),
            );

            if (prices.size === 0) {
                logger.error('No prices fetched from any provider');
                assetsFailed = assets.length;
                contractUpdateOk = false;
            } else {
                const results = await this.contractUpdater.updatePrices(
                    Array.from(prices.values()),
                );
                const failed = results.filter((r) => !r.success);
                contractUpdateOk = failed.length === 0;

                if (failed.length > 0) {
                    logger.warn('Some contract price updates failed', {
                        failedAssets: failed.map((f) => ({ asset: f.asset, error: f.error })),
                    });
                }
            }
        } catch (err) {
            logger.error('Price update cycle threw unexpectedly', {
                errorClass: 'unknown',
            });
            contractUpdateOk = false;
        }

        const diagnostics: UpdateCycleDiagnostics = {
            startedAt,
            durationMs: Date.now() - cycleStart,
            assetsUpdated,
            assetsFailed,
            providerEvents: allProviderEvents,
            cooledDownProviders: Array.from(cooledDownProviders),
            contractUpdateOk,
        };

        this.telemetry.recordCycle(diagnostics);
    }

    // ─── Queries ───────────────────────────────────────────────────────────────

    getStatus() {
        return {
            isRunning: this.isRunning,
            network: this.config.stellarNetwork,
            contractId: this.config.contractId,
            providers: this.aggregator.getProviders(),
            aggregatorStats: this.aggregator.getStats(),
        };
    }

    getTelemetry(): TelemetrySummary {
        return this.telemetry.getSummary();
    }

    async fetchPrice(asset: string) {
        return this.aggregator.getPrice(asset);
    }
}

// ─── Main entry point ─────────────────────────────────────────────────────────

async function main(): Promise<void> {
    console.log(`
╔═══════════════════════════════════════════════════════════╗
║                StellarLend Oracle Service                  ║
║                                                            ║
║  Off-chain oracle integration for price data management   ║
╚═══════════════════════════════════════════════════════════╝
  `);

    try {
        const config = loadConfig();
        const service = new OracleService(config);

        process.on('SIGINT', () => {
            logger.info('Received SIGINT, shutting down…');
            service.stop();
            process.exit(0);
        });

        process.on('SIGTERM', () => {
            logger.info('Received SIGTERM, shutting down…');
            service.stop();
            process.exit(0);
        });

        await service.start();
    } catch (error) {
        console.error('Failed to start oracle service:', error);
        process.exit(1);
    }
}

main().catch(console.error);

export { loadConfig } from './config.js';
export type { OracleServiceConfig } from './config.js';
