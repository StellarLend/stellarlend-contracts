/**
 * Price Aggregator Service
 *
 * Fetches prices from multiple providers and aggregates them using
 * weighted-median calculation with MAD outlier rejection.
 */

import type {
    PriceData,
    AggregatedPrice,
    ProviderFetchEvent,
} from '../types/index.js';
import { BasePriceProvider } from '../providers/base-provider.js';
import { PriceValidator } from './price-validator.js';
import { PriceCache } from './cache.js';
import { MAD_Z_SCORE_THRESHOLD, runtimeConfig } from '../config.js';
import { logger } from '../utils/logger.js';

// ─── Configuration ────────────────────────────────────────────────────────────

export interface AggregatorConfig {
    minSources: number;
    useWeightedMedian: boolean;
    /** MAD z-score threshold; prices beyond this are rejected as outliers (0 = disabled). */
    madZScoreThreshold: number;
    /** Max age of a cached price in milliseconds before triggering a refresh. */
    maxStalenessMs: number;
    /** Max age of a stale cached price that may be served as fallback if refresh fails. */
    maxFallbackAgeMs: number;
    /** Per-provider retry attempts on transient failure (0 = no retry). */
    providerRetries: number;
    /** Base back-off in milliseconds between per-provider retries. */
    retryBackoffMs: number;
    /**
     * Maximum number of provider fetches that may run concurrently within a
     * single `fetchWithFallback` call.  Bounds parallelism without serialising.
     */
    maxConcurrentProviders: number;
}

const DEFAULT_CONFIG: Required<AggregatorConfig> = {
    minSources: 1,
    useWeightedMedian: true,
    madZScoreThreshold: MAD_Z_SCORE_THRESHOLD,
    maxStalenessMs: 30_000,
    maxFallbackAgeMs: 300_000,
    providerRetries: 2,
    retryBackoffMs: 100,
    maxConcurrentProviders: runtimeConfig.maxConcurrentProviders,
};

// ─── PriceAggregator ──────────────────────────────────────────────────────────

export class PriceAggregator {
    private providers: BasePriceProvider[];
    private validator: PriceValidator;
    private cache: PriceCache;
    private config: Required<AggregatorConfig>;
    /** Tracks when each asset was last written to cache (ms). */
    private cacheTimestamps: Map<string, number> = new Map();
    /** In-flight requests per asset — coalesces concurrent callers. */
    private pendingRequests: Map<string, Promise<AggregatedPrice | null>> = new Map();

    constructor(
        providers: BasePriceProvider[],
        validator: PriceValidator,
        cache: PriceCache,
        config: Partial<AggregatorConfig> = {},
    ) {
        this.providers = providers
            .filter((p) => p.isEnabled)
            .sort((a, b) => a.priority - b.priority);

        this.validator = validator;
        this.cache = cache;
        this.config = { ...DEFAULT_CONFIG, ...config } as Required<AggregatorConfig>;

        if (this.config.minSources < 1) throw new Error('minSources must be at least 1');

        logger.info('Price aggregator initialized', {
            enabledProviders: this.providers.map((p) => p.name),
            minSources: this.config.minSources,
            maxConcurrentProviders: this.config.maxConcurrentProviders,
        });
    }

    // ─── Public API ───────────────────────────────────────────────────────────

    /**
     * Fetch and aggregate price for a single asset.
     *
     * - Returns the cached result when it is still fresh (age < maxStalenessMs).
     * - Coalesces concurrent calls for the same asset into a single in-flight request.
     * - Falls back to a stale cached value when a refresh fails and the stale value
     *   is still within maxFallbackAgeMs.
     */
    async getPrice(asset: string): Promise<AggregatedPrice | null> {
        const upperAsset = asset.toUpperCase();
        const now = Date.now();

        const cachedPrice = this.cache.getPrice(upperAsset);
        const cachedAt = this.cacheTimestamps.get(upperAsset);

        if (cachedPrice !== undefined && cachedAt !== undefined) {
            const ageMs = now - cachedAt;
            if (ageMs >= 0 && ageMs < this.config.maxStalenessMs) {
                logger.debug(`Using fresh cached price for ${upperAsset}`, { ageMs });
                return this.formatCachedPrice(upperAsset, cachedPrice, cachedAt);
            }
        }

        // Coalesce: reuse an in-flight request rather than launching a duplicate.
        const pending = this.pendingRequests.get(upperAsset);
        if (pending) {
            logger.debug(`Reusing in-flight price request for ${upperAsset}`);
            return pending;
        }

        const request = this.refreshPrice(upperAsset, cachedPrice, cachedAt);
        this.pendingRequests.set(upperAsset, request);

        try {
            return await request;
        } finally {
            if (this.pendingRequests.get(upperAsset) === request) {
                this.pendingRequests.delete(upperAsset);
            }
        }
    }

    /**
     * Fetch prices for multiple assets concurrently (bounded by aggregator config).
     */
    async getPrices(assets: string[]): Promise<Map<string, AggregatedPrice>> {
        const results = new Map<string, AggregatedPrice>();

        await Promise.allSettled(
            assets.map(async (asset) => {
                const price = await this.getPrice(asset);
                if (price) results.set(asset.toUpperCase(), price);
            }),
        );

        return results;
    }

    /**
     * Fetch price from providers with bounded concurrency and per-provider telemetry.
     *
     * At most `maxConcurrentProviders` providers run simultaneously per batch.
     * Cooled-down providers are skipped and recorded in telemetry.
     * Each provider attempt records a ProviderFetchEvent for diagnostics.
     */
    async fetchWithFallback(
        asset: string,
    ): Promise<{ prices: PriceData[]; events: ProviderFetchEvent[] }> {
        const validPrices: PriceData[] = [];
        const events: ProviderFetchEvent[] = [];

        const concurrency = Math.max(1, this.config.maxConcurrentProviders);

        for (let i = 0; i < this.providers.length; i += concurrency) {
            const batch = this.providers.slice(i, i + concurrency);

            const batchResults = await Promise.allSettled(
                batch.map(async (provider) => {
                    if (provider.isCooledDown) {
                        logger.warn(`Skipping ${provider.name}: active cooldown`);
                        events.push({ provider: provider.name, asset, latencyMs: 0, success: false, errorClass: 'rate_limit' });
                        return null;
                    }

                    const maxAttempts = this.config.providerRetries + 1;

                    for (let attempt = 1; attempt <= maxAttempts; attempt++) {
                        const start = Date.now();
                        try {
                            const rawPrice = await provider.fetchPrice(asset);
                            const latencyMs = Date.now() - start;
                            const validation = this.validator.validate(rawPrice);

                            if (validation.isValid && validation.price) {
                                events.push({ provider: provider.name, asset, latencyMs, success: true });
                                logger.debug(`Valid price from ${provider.name} for ${asset}`, {
                                    price: validation.price.price.toString(),
                                    latencyMs,
                                });
                                return validation.price;
                            }

                            events.push({ provider: provider.name, asset, latencyMs: Date.now() - start, success: false, errorClass: 'validation' });
                            logger.warn(`Invalid price from ${provider.name} for ${asset}`, { errors: validation.errors });
                            return null; // validation failure — no retry
                        } catch (err) {
                            const latencyMs = Date.now() - start;
                            const errClass = classifyFetchError(err);

                            if (attempt < maxAttempts) {
                                const backoffMs = this.config.retryBackoffMs * attempt;
                                logger.warn(`${provider.name} failed for ${asset}; retry ${attempt}/${maxAttempts - 1} in ${backoffMs}ms`, { errorClass: errClass });
                                await new Promise((r) => setTimeout(r, backoffMs));
                                continue;
                            }

                            events.push({ provider: provider.name, asset, latencyMs, success: false, errorClass: errClass });
                            logger.warn(`${provider.name} failed for ${asset} after ${maxAttempts} attempt(s)`, {
                                errorClass: errClass,
                                // err.message omitted — may contain secrets from RPC responses
                            });
                            return null;
                        }
                    }

                    return null;
                }),
            );

            for (const r of batchResults) {
                if (r.status === 'fulfilled' && r.value !== null) {
                    validPrices.push(r.value);
                }
            }
        }

        if (validPrices.length === 0) {
            logger.error(`All providers failed for ${asset}`, {
                providers: this.providers.map((p) => p.name),
            });
        }

        return { prices: validPrices, events };
    }

    /**
     * Aggregate pre-fetched `prices` for `asset`, write to cache, return result.
     * Lets callers (e.g. OracleService) avoid a second provider round-trip after
     * having already called `fetchWithFallback` to collect telemetry.
     */
    aggregateAndCache(asset: string, prices: PriceData[]): AggregatedPrice | null {
        if (prices.length < this.config.minSources) return null;
        const upper = asset.toUpperCase();
        const aggregated = this.aggregate(upper, prices);
        this.cache.setPrice(upper, aggregated.price);
        this.cacheTimestamps.set(upper, Date.now());
        return aggregated;
    }

    getProviders(): string[] {
        return this.providers.map((p) => p.name);
    }

    getStats() {
        return {
            enabledProviders: this.providers.length,
            cacheStats: this.cache.getStats(),
        };
    }

    // ─── Private helpers ──────────────────────────────────────────────────────

    /**
     * Refresh price from providers, write cache on success, fall back to stale
     * cache value when refresh fails and the stale value is within maxFallbackAgeMs.
     */
    private async refreshPrice(
        asset: string,
        cachedPrice: bigint | undefined,
        cachedAt: number | undefined,
    ): Promise<AggregatedPrice | null> {
        const { prices: validPrices } = await this.fetchWithFallback(asset);

        if (validPrices.length >= this.config.minSources) {
            const aggregated = this.aggregate(asset, validPrices);
            this.cache.setPrice(asset, aggregated.price);
            this.cacheTimestamps.set(asset, Date.now());
            return aggregated;
        }

        if (cachedPrice !== undefined && cachedAt !== undefined) {
            const ageMs = Date.now() - cachedAt;
            if (ageMs >= 0 && ageMs < this.config.maxFallbackAgeMs) {
                logger.warn(`Returning stale cached price for ${asset} after refresh failure`, {
                    ageMs,
                    validSources: validPrices.length,
                });
                const confidence = Math.max(
                    0,
                    Math.round(100 * (1 - ageMs / this.config.maxFallbackAgeMs)),
                );
                return this.formatCachedPrice(asset, cachedPrice, cachedAt, confidence);
            }
        }

        logger.error(`No usable price for ${asset}`, {
            got: validPrices.length,
            required: this.config.minSources,
        });
        return null;
    }

    private formatCachedPrice(
        asset: string,
        price: bigint,
        timestamp: number,
        confidence = 100,
    ): AggregatedPrice {
        return {
            asset,
            price,
            sources: [],
            timestamp: Math.floor(timestamp / 1000),
            confidence,
        };
    }

    private aggregate(asset: string, prices: PriceData[]): AggregatedPrice {
        const now = Math.floor(Date.now() / 1000);

        if (prices.length === 1) {
            return {
                asset,
                price: prices[0].price,
                sources: prices,
                timestamp: now,
                confidence: prices[0].confidence,
            };
        }

        const filtered = filterOutliersByMAD(prices, this.config.madZScoreThreshold);
        const activePrices = filtered.length >= this.config.minSources ? filtered : prices;

        if (filtered.length < prices.length) {
            logger.warn(`MAD filter removed ${prices.length - filtered.length} outlier(s) for ${asset}`, {
                removed: prices
                    .filter((p) => !filtered.includes(p))
                    .map((p) => ({ source: p.source, price: p.price.toString() })),
            });
        }

        const aggregatedPrice = this.config.useWeightedMedian
            ? this.weightedMedian(activePrices)
            : this.simpleMedian(activePrices);

        const totalWeight = activePrices.reduce((sum, p) => sum + this.getSourceWeight(p), 0);
        const weightedConfidence =
            totalWeight > 0 && Number.isFinite(totalWeight)
                ? activePrices.reduce((sum, p) => sum + p.confidence * this.getSourceWeight(p), 0) / totalWeight
                : activePrices.reduce((sum, p) => sum + p.confidence, 0) / activePrices.length;

        return {
            asset,
            price: aggregatedPrice,
            sources: activePrices,
            timestamp: now,
            confidence: Math.round(weightedConfidence),
        };
    }

    private weightedMedian(prices: PriceData[]): bigint {
        const sorted = [...prices].sort((a, b) =>
            a.price < b.price ? -1 : a.price > b.price ? 1 : 0,
        );
        const weights = sorted.map((p) => this.getSourceWeight(p));
        const totalWeight = weights.reduce((a, b) => a + b, 0);
        const halfWeight = totalWeight / 2;

        let cumWeight = 0;
        for (let i = 0; i < sorted.length; i++) {
            cumWeight += weights[i];
            if (cumWeight >= halfWeight) return sorted[i].price;
        }
        return sorted[sorted.length - 1].price;
    }

    private simpleMedian(prices: PriceData[]): bigint {
        const sorted = [...prices].sort((a, b) =>
            a.price < b.price ? -1 : a.price > b.price ? 1 : 0,
        );
        const mid = Math.floor(sorted.length / 2);
        if (sorted.length % 2 === 0) return (sorted[mid - 1].price + sorted[mid].price) / 2n;
        return sorted[mid].price;
    }

    /**
     * Numeric weight for a price point: volume24h when available, else provider weight.
     * Keeps confidence weighting consistent with weighted-median price selection.
     */
    private getSourceWeight(price: PriceData): number {
        if (price.volume24h !== undefined && price.volume24h > 0n) {
            return Number(price.volume24h);
        }
        const provider = this.providers.find((pr) => pr.name === price.source);
        return provider?.weight ?? 0.1;
    }
}

// ─── Factory ──────────────────────────────────────────────────────────────────

export function createAggregator(
    providers: BasePriceProvider[],
    validator: PriceValidator,
    cache: PriceCache,
    config?: Partial<AggregatorConfig>,
): PriceAggregator {
    return new PriceAggregator(providers, validator, cache, config);
}

// ─── Error classification ─────────────────────────────────────────────────────

type FetchErrorClass = ProviderFetchEvent['errorClass'];

function classifyFetchError(err: unknown): FetchErrorClass {
    if (!(err instanceof Error)) return 'unknown';
    const msg = err.message.toLowerCase();
    if (msg.includes('timeout') || msg.includes('timed out')) return 'timeout';
    if (msg.includes('429') || msg.includes('rate limit') || msg.includes('too many')) return 'rate_limit';
    if (msg.includes('invalid') || msg.includes('validation') || msg.includes('malformed')) return 'validation';
    if (msg.includes('network') || msg.includes('econnrefused') || msg.includes('enotfound') || msg.includes('socket')) return 'network';
    return 'unknown';
}

// ─── MAD outlier filter ───────────────────────────────────────────────────────

/**
 * Filter outlier prices using the Median Absolute Deviation (MAD) method.
 *
 * Modified z-score: z_i = |p_i - median| / (1.4826 * MAD).
 * Prices with z_i > zMax are rejected.
 *
 * Special cases: ≤2 prices, MAD=0, or zMax≤0 → return all.
 */
export function filterOutliersByMAD(prices: PriceData[], zMax: number): PriceData[] {
    if (zMax <= 0 || prices.length <= 2) return prices;

    const sorted = [...prices].map((p) => p.price).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
    const med = bigintMedian(sorted);
    const deviations = sorted.map((p) => (p > med ? p - med : med - p));
    const mad = bigintMedian([...deviations].sort((a, b) => (a < b ? -1 : a > b ? 1 : 0)));

    if (mad === 0n) return prices;

    const zMaxScaled = BigInt(Math.round(zMax * 14826));
    return prices.filter((p) => {
        const dev = p.price > med ? p.price - med : med - p.price;
        return dev * 10000n <= zMaxScaled * mad;
    });
}

function bigintMedian(sorted: bigint[]): bigint {
    const mid = Math.floor(sorted.length / 2);
    if (sorted.length % 2 === 1) return sorted[mid];
    return (sorted[mid - 1] + sorted[mid]) / 2n;
}
