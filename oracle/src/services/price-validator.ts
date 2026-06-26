/**
 * Price Validator Service
 * 
 * Validates and sanitizes price data before it's used for
 * contract updates. Implements multiple validation checks:
*/

import type {
    RawPriceData,
    PriceData,
    ValidationResult,
    ValidationError,
    ValidationErrorCode,
    AssetPriceBounds,
} from '../types/index.js';
import { scalePrice } from '../config.js';
import { logger } from '../utils/logger.js';

/**
 * Validator configuration
 */
export interface ValidatorConfig {
    maxDeviationPercent: number;
    maxStalenessSeconds: number;
    minPrice: number;
    maxPrice: number;
}

/**
 * Default validator configuration
 */
const DEFAULT_CONFIG: ValidatorConfig = {
    maxDeviationPercent: 10,
    maxStalenessSeconds: 300,
    minPrice: 0.0000001,
    maxPrice: 1000000000,
};

/**
 * Price Validator
 */
export class PriceValidator {
    private config: ValidatorConfig;
    private cachedPrices: Map<string, number> = new Map();
    private assetBounds: Record<string, AssetPriceBounds>;

    constructor(
        config: Partial<ValidatorConfig> = {},
        assetBounds: Record<string, AssetPriceBounds> = {},
    ) {
        this.config = { ...DEFAULT_CONFIG, ...config };
        this.assetBounds = this.normalizeBounds(assetBounds);

        logger.info('Price validator initialized', {
            maxDeviationPercent: this.config.maxDeviationPercent,
            maxStalenessSeconds: this.config.maxStalenessSeconds,
            assetBounds: Object.keys(this.assetBounds).length,
        });
    }

    /**
     * Validate raw price data and convert to validated PriceData
     */
    validate(raw: RawPriceData): ValidationResult {
        const errors: ValidationError[] = [];

        if (raw.price <= 0) {
            errors.push({
                code: 'PRICE_ZERO' as ValidationErrorCode,
                message: `Price must be positive, got ${raw.price}`,
            });
        }

        if (raw.price < this.config.minPrice) {
            errors.push({
                code: 'PRICE_ZERO' as ValidationErrorCode,
                message: `Price ${raw.price} below minimum ${this.config.minPrice}`,
            });
        }

        if (raw.price > this.config.maxPrice) {
            errors.push({
                code: 'PRICE_DEVIATION_TOO_HIGH' as ValidationErrorCode,
                message: `Price ${raw.price} exceeds maximum ${this.config.maxPrice}`,
            });
        }

        const now = Math.floor(Date.now() / 1000);
        const age = now - raw.timestamp;

        if (age > this.config.maxStalenessSeconds) {
            errors.push({
                code: 'PRICE_STALE' as ValidationErrorCode,
                message: `Price is ${age}s old, max allowed is ${this.config.maxStalenessSeconds}s`,
                details: { age, maxAge: this.config.maxStalenessSeconds },
            });
        }

        const asset = raw.asset.toUpperCase();
        const bounds = this.getBounds(asset);

        if (raw.price < bounds.minPrice) {
            errors.push({
                code: 'PRICE_BELOW_MIN' as ValidationErrorCode,
                message: `Price ${raw.price} below minimum ${bounds.minPrice} for ${asset}`,
                details: {
                    asset,
                    minPrice: bounds.minPrice,
                },
            });
        }

        if (raw.price > bounds.maxPrice) {
            errors.push({
                code: 'PRICE_ABOVE_MAX' as ValidationErrorCode,
                message: `Price ${raw.price} exceeds maximum ${bounds.maxPrice} for ${asset}`,
                details: {
                    asset,
                    maxPrice: bounds.maxPrice,
                },
            });
        }

        const cachedPrice = this.cachedPrices.get(asset);
        if (cachedPrice !== undefined) {
            const deviation = Math.abs((raw.price - cachedPrice) / cachedPrice) * 100;

            if (deviation > this.config.maxDeviationPercent) {
                errors.push({
                    code: 'PRICE_DEVIATION_TOO_HIGH' as ValidationErrorCode,
                    message: `Price deviation ${deviation.toFixed(2)}% exceeds max ${this.config.maxDeviationPercent}%`,
                    details: {
                        newPrice: raw.price,
                        cachedPrice,
                        deviationPercent: deviation,
                    },
                });
            }
        }

        if (errors.length === 0) {
            const validatedPrice: PriceData = {
                asset,
                price: scalePrice(raw.price),
                timestamp: raw.timestamp,
                source: raw.source,
                confidence: this.calculateConfidence(raw, cachedPrice),
                volume24h: raw.volume24h,
            };

            this.cachedPrices.set(asset, raw.price);

            return {
                isValid: true,
                price: validatedPrice,
                errors: [],
            };
        }

        logger.warn(`Price validation failed for ${raw.asset}`, { errors });

        return {
            isValid: false,
            errors,
        };
    }

    /**
     * Validate multiple prices
     */
    validateMany(prices: RawPriceData[]): ValidationResult[] {
        return prices.map((p) => this.validate(p));
    }

    /**
     * Calculate confidence score based on various factors
     */
    private calculateConfidence(raw: RawPriceData, cachedPrice?: number): number {
        let confidence = 100;

        const now = Math.floor(Date.now() / 1000);
        const age = now - raw.timestamp;
        const ageRatio = age / this.config.maxStalenessSeconds;
        confidence -= Math.min(20, ageRatio * 20);

        if (cachedPrice !== undefined) {
            const deviation = Math.abs((raw.price - cachedPrice) / cachedPrice) * 100;
            const deviationRatio = deviation / this.config.maxDeviationPercent;
            confidence -= Math.min(30, deviationRatio * 30);
        }

        switch (raw.source) {


            case 'coingecko':
                confidence += 0;
                break;
            case 'binance':
                confidence -= 5;
                break;
        }

        return Math.max(0, Math.min(100, confidence));
    }

    /**
     * Update cached price manually (e.g., after successful contract update)
     */
    updateCache(asset: string, price: number): void {
        this.cachedPrices.set(asset.toUpperCase(), price);
    }

    /**
     * Reload validator settings and optional bounds at runtime
     */
    reloadConfig(
        config: Partial<ValidatorConfig> = {},
        assetBounds?: Record<string, AssetPriceBounds>,
    ): void {
        this.config = { ...this.config, ...config };

        if (assetBounds) {
            this.assetBounds = this.normalizeBounds(assetBounds);
        }

        logger.info('Price validator configuration reloaded', {
            maxDeviationPercent: this.config.maxDeviationPercent,
            maxStalenessSeconds: this.config.maxStalenessSeconds,
            boundsUpdated: assetBounds ? Object.keys(assetBounds).length : 0,
        });
    }

    /**
     * Clear cached price for an asset
     */
    clearCache(asset?: string): void {
        if (asset) {
            this.cachedPrices.delete(asset.toUpperCase());
        } else {
            this.cachedPrices.clear();
        }
    }

    /**
     * Get current cache state (for debugging)
     */
    getCacheState(): Record<string, number> {
        return Object.fromEntries(this.cachedPrices);
    }

    private getBounds(asset: string): AssetPriceBounds {
        return this.assetBounds[asset] ?? {
            minPrice: this.config.minPrice,
            maxPrice: this.config.maxPrice,
        };
    }

    private normalizeBounds(
        bounds: Record<string, AssetPriceBounds>,
    ): Record<string, AssetPriceBounds> {
        return Object.fromEntries(
            Object.entries(bounds).map(([asset, value]) => [
                asset.toUpperCase(),
                {
                    minPrice: value.minPrice,
                    maxPrice: value.maxPrice,
                },
            ]),
        );
    }
}

/**
 * Create a validator with custom configuration
 */
export function createValidator(
    config?: Partial<ValidatorConfig>,
    assetBounds: Record<string, AssetPriceBounds> = {},
): PriceValidator {
    return new PriceValidator(config, assetBounds);
}
