/**
 * Price Aggregator Service
 * 
 * Fetches prices from multiple providers and aggregates them
 * using weighted median calculation.
 */

import type {
    RawPriceData,
    PriceData,
    AggregatedPrice,
} from '../types/index.js';
import { BasePriceProvider } from '../providers/base-provider.js';
import { PriceValidator } from './price-validator.js';
import { PriceCache } from './cache.js';
import { scalePrice } from '../config.js';
import { logger } from '../utils/logger.js';

/**
 * Aggregator configuration
 */
export interface AggregatorConfig {
    minSources: number;
    useWeightedMedian: boolean;
}

/**
 * Default aggregator configuration
 */
const DEFAULT_CONFIG: AggregatorConfig = {
    minSources: 1,
    useWeightedMedian: true,
};

/**
 * Price Aggregator
 */
export class PriceAggregator {
    private providers: BasePriceProvider[];
    private validator: PriceValidator;
    private cache: PriceCache;
    private config: AggregatorConfig;

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
        this.config = { ...DEFAULT_CONFIG, ...config };

        logger.info('Price aggregator initialized', {
            enabledProviders: this.providers.map((p) => p.name),
            minSources: this.config.minSources,
        });
    }

    /**
     * Fetch and aggregate price for a single asset
     */
    async getPrice(asset: string): Promise<AggregatedPrice | null> {
        const upperAsset = asset.toUpperCase();

        const cachedPrice = this.cache.getPrice(upperAsset);
        if (cachedPrice !== undefined) {
            logger.debug(`Using cached price for ${upperAsset}`);
            return {
                asset: upperAsset,
                price: cachedPrice,
                sources: [],
                timestamp: Math.floor(Date.now() / 1000),
                confidence: 100,
            };
        }

        const validPrices = await this.fetchWithFallback(upperAsset);

        if (validPrices.length < this.config.minSources) {
            logger.error(`Not enough valid sources for ${upperAsset}`, {
                got: validPrices.length,
                required: this.config.minSources,
            });
            return null;
        }

        const aggregated = this.aggregate(upperAsset, validPrices);

        this.cache.setPrice(upperAsset, aggregated.price);

        return aggregated;
    }

    /**
     * Fetch prices for multiple assets
     */
    async getPrices(assets: string[]): Promise<Map<string, AggregatedPrice>> {
        const results = new Map<string, AggregatedPrice>();

        const promises = assets.map(async (asset) => {
            const price = await this.getPrice(asset);
            if (price) {
                results.set(asset.toUpperCase(), price);
            }
        });

        await Promise.allSettled(promises);

        return results;
    }

    /**
     * Fetch price from providers with fallback logic
     */
    private async fetchWithFallback(asset: string): Promise<PriceData[]> {
        const validPrices: PriceData[] = [];
        const errors: Map<string, Error> = new Map();

        for (const provider of this.providers) {
            try {
                const rawPrice = await provider.fetchPrice(asset);
                const validation = this.validator.validate(rawPrice);

                if (validation.isValid && validation.price) {
                    validPrices.push(validation.price);
                    logger.debug(`Got valid price from ${provider.name} for ${asset}`, {
                        price: validation.price.price.toString(),
                    });
                } else {
                    logger.warn(`Invalid price from ${provider.name} for ${asset}`, {
                        errors: validation.errors,
                    });
                }
            } catch (error) {
                errors.set(provider.name, error instanceof Error ? error : new Error(String(error)));
                logger.warn(`Provider ${provider.name} failed for ${asset}`, { error });
            }
        }

        if (validPrices.length === 0 && errors.size > 0) {
            logger.error(`All providers failed for ${asset}`, {
                providers: Array.from(errors.keys()),
            });
        }

        return validPrices;
    }

    /**
     * Aggregate prices from multiple sources
     */
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

        const aggregatedPrice = this.config.useWeightedMedian
            ? this.weightedMedian(prices)
            : this.simpleMedian(prices);

        const totalWeight = this.providers
            .filter((p) => prices.some((pr) => pr.source === p.name))
            .reduce((sum, p) => sum + p.weight, 0);

        const weightedConfidence = prices.reduce((sum, p) => {
            const provider = this.providers.find((pr) => pr.name === p.source);
            const weight = provider?.weight ?? 0.1;
            return sum + (p.confidence * weight);
        }, 0) / totalWeight;

        return {
            asset,
            price: aggregatedPrice,
            sources: prices,
            timestamp: now,
            confidence: Math.round(weightedConfidence),
        };
    }

    /**
     * Calculate weighted median of prices
     */
    private weightedMedian(prices: PriceData[]): bigint {
        const sorted = [...prices].sort((a, b) =>
            a.price < b.price ? -1 : a.price > b.price ? 1 : 0
        );

        const weights = sorted.map((p) => {
            const provider = this.providers.find((pr) => pr.name === p.source);
            return provider?.weight ?? 0.1;
        });

        const totalWeight = weights.reduce((a, b) => a + b, 0);
        const halfWeight = totalWeight / 2;

        let cumWeight = 0;
        for (let i = 0; i < sorted.length; i++) {
            cumWeight += weights[i];
            if (cumWeight >= halfWeight) {
                return sorted[i].price;
            }
        }

        return sorted[sorted.length - 1].price;
    }

    /**
     * Calculate simple median of prices
     */
    private simpleMedian(prices: PriceData[]): bigint {
        const sorted = [...prices].sort((a, b) =>
            a.price < b.price ? -1 : a.price > b.price ? 1 : 0
        );

        const mid = Math.floor(sorted.length / 2);

        if (sorted.length % 2 === 0) {
            const avg = (sorted[mid - 1].price + sorted[mid].price) / 2n;
            return avg;
        }

        return sorted[mid].price;
    }

    /**
     * Get list of enabled providers
     */
    getProviders(): string[] {
        return this.providers.map((p) => p.name);
    }

    /**
     * Get aggregator statistics
     */
    getStats() {
        return {
            enabledProviders: this.providers.length,
            cacheStats: this.cache.getStats(),
        };
    }
}

/**
 * Create a price aggregator
 */
export function createAggregator(
    providers: BasePriceProvider[],
    validator: PriceValidator,
    cache: PriceCache,
    config?: Partial<AggregatorConfig>,
): PriceAggregator {
    return new PriceAggregator(providers, validator, cache, config);
}
