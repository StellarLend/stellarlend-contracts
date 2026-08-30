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
import { Keypair } from '@stellar/stellar-sdk';
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
    /**
     * Maximum age in seconds for a cached price to be used as a fallback when the
     * live price is rejected for staleness. When not configured, it defaults to
     * three times maxStalenessSeconds.
     */
    maxFallbackStalenessSeconds?: number;
}

/**
 * Cached price entry with the source timestamp used for freshness checks.
 */
interface CachedPrice {
    price: number;
    timestamp: number;
    volume24h?: number;
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
interface CachedPrice {
    price: number;
    timestamp: number;
}

interface PendingPrice {
    price: number;
    timestamp: number;
}

export class PriceValidator {
    private config: ValidatorConfig;
    private cachedPrices: Map<string, CachedPrice> = new Map();
    private pendingPrices: Map<string, PendingPrice> = new Map();
    private assetBounds: Record<string, AssetPriceBounds>;
    private trustedSigners: Record<string, string[]>;
    private signatureDomain: string;

    constructor(
        config: Partial<ValidatorConfig> = {},
        assetBounds: Record<string, AssetPriceBounds> = {},
        trustedSigners: Record<string, string[]> = {},
        signatureDomain: string = 'StellarLendOracle',
    ) {
        const mergedConfig = { ...DEFAULT_CONFIG, ...config };
        const maxStalenessSeconds = mergedConfig.maxStalenessSeconds;
        this.config = {
            ...mergedConfig,
            maxFallbackStalenessSeconds:
                mergedConfig.maxFallbackStalenessSeconds ?? maxStalenessSeconds * 3,
        };
        this.validateConfig(this.config);
        this.assetBounds = this.normalizeBounds(assetBounds);
        this.trustedSigners = trustedSigners;
        this.signatureDomain = signatureDomain;

        logger.info('Price validator initialized', {
            maxDeviationPercent: this.config.maxDeviationPercent,
            maxStalenessSeconds: this.config.maxStalenessSeconds,
            maxFallbackStalenessSeconds: this.getFallbackStalenessSeconds(),
            assetBounds: Object.keys(this.assetBounds).length,
        });
    }

    /**
     * Validate raw price data and convert to validated PriceData
     */
    validate(raw: RawPriceData): ValidationResult {
        const errors: ValidationError[] = [];

        if (!Number.isFinite(raw.price) || raw.price <= 0) {
            errors.push({
                code: 'PRICE_ZERO' as ValidationErrorCode,
                message: `Price must be a positive finite number, got ${raw.price}`,
            });
        }

        if (Number.isFinite(raw.price) && raw.price < this.config.minPrice) {
            errors.push({
                code: 'PRICE_ZERO' as ValidationErrorCode,
                message: `Price ${raw.price} below minimum ${this.config.minPrice}`,
            });
        }

        if (Number.isFinite(raw.price) && raw.price > this.config.maxPrice) {
            errors.push({
                code: 'PRICE_DEVIATION_TOO_HIGH' as ValidationErrorCode,
                message: `Price ${raw.price} exceeds maximum ${this.config.maxPrice}`,
            });
        }

        const now = Math.floor(Date.now() / 1000);
        const age = now - raw.timestamp;

        if (age < 0) {
            errors.push({
                code: 'PRICE_STALE' as ValidationErrorCode,
                message: `Price timestamp ${raw.timestamp} is in the future`,
                details: { timestamp: raw.timestamp, now },
            });
        }

        if (age > this.config.maxStalenessSeconds) {
            errors.push({
                code: 'PRICE_STALE' as ValidationErrorCode,
                message: `Price is ${age}s old, max allowed is ${this.config.maxStalenessSeconds}s`,
                details: { age, maxAge: this.config.maxStalenessSeconds },
            });
        }

        const asset = raw.asset.toUpperCase();
        const bounds = this.getBounds(asset);

        if (Number.isFinite(raw.price) && raw.price < bounds.minPrice) {
            errors.push({
                code: 'PRICE_BELOW_MIN' as ValidationErrorCode,
                message: `Price ${raw.price} below minimum ${bounds.minPrice} for ${asset}`,
                details: {
                    asset,
                    minPrice: bounds.minPrice,
                },
            });
        }

        if (Number.isFinite(raw.price) && raw.price > bounds.maxPrice) {
            errors.push({
                code: 'PRICE_ABOVE_MAX' as ValidationErrorCode,
                message: `Price ${raw.price} exceeds maximum ${bounds.maxPrice} for ${asset}`,
                details: {
                    asset,
                    maxPrice: bounds.maxPrice,
                },
            });
        }

        const pending = this.pendingPrices.get(asset);
        const cached = this.cachedPrices.get(asset);
        const cachedPrice = cached?.price;

        if (pending !== undefined) {
            if (pending.price === raw.price && pending.timestamp === raw.timestamp) {
                errors.push({
                    code: 'PRICE_STALE' as ValidationErrorCode,
                    message: `Duplicate price submission for ${asset} is already pending; commit or rollback before retrying`,
                    details: { asset, price: raw.price, timestamp: raw.timestamp },
                });
            } else {
                errors.push({
                    code: 'PRICE_STALE' as ValidationErrorCode,
                    message: `Conflicting price submission for ${asset}; a different price is already pending`,
                    details: {
                        asset,
                        pendingPrice: pending.price,
                        pendingTimestamp: pending.timestamp,
                        newPrice: raw.price,
                        newTimestamp: raw.timestamp,
                    },
                });
            }
        }

        if (cached !== undefined) {
            if (raw.timestamp < cached.timestamp) {
                errors.push({
                    code: 'PRICE_STALE' as ValidationErrorCode,
                    message: `Price timestamp ${raw.timestamp} is older than last accepted timestamp ${cached.timestamp} for ${asset}`,
                    details: {
                        asset,
                        timestamp: raw.timestamp,
                        lastTimestamp: cached.timestamp,
                    },
                });
            } else if (raw.timestamp === cached.timestamp && raw.price === cached.price) {
                errors.push({
                    code: 'PRICE_STALE' as ValidationErrorCode,
                    message: `Price for ${asset} at timestamp ${raw.timestamp} is already committed`,
                    details: { asset, timestamp: raw.timestamp, price: raw.price },
                });
            } else if (raw.timestamp === cached.timestamp) {
                errors.push({
                    code: 'PRICE_STALE' as ValidationErrorCode,
                    message: `Price for timestamp ${raw.timestamp} conflicts with last accepted price ${cached.price} for ${asset}`,
                    details: {
                        asset,
                        timestamp: raw.timestamp,
                        cachedPrice: cached.price,
                        price: raw.price,
                    },
                });
            }

            const deviation = Math.abs((raw.price - cached.price) / cached.price) * 100;

            if (deviation > this.config.maxDeviationPercent) {
                errors.push({
                    code: 'PRICE_DEVIATION_TOO_HIGH' as ValidationErrorCode,
                    message: `Price deviation ${deviation.toFixed(2)}% exceeds max ${this.config.maxDeviationPercent}%`,
                    details: {
                        newPrice: raw.price,
                        cachedPrice: cached.price,
                        deviationPercent: deviation,
                    },
                });
            }
        }

        const scaledPrice = scalePrice(raw.price);
        if (Number.isFinite(raw.price) && !Number.isSafeInteger(scaledPrice)) {
            errors.push({
                code: 'PRICE_DEVIATION_TOO_HIGH' as ValidationErrorCode,
                message: `Scaled price ${scaledPrice} for ${asset} is not a safe integer`,
                details: { scaledPrice, maxSafeInteger: Number.MAX_SAFE_INTEGER },
            });
        }

        if (errors.length === 0) {
            const validatedPrice: PriceData = {
                asset,
                price: scaledPrice,
                timestamp: raw.timestamp,
                source: raw.source,
                confidence: this.calculateConfidence(raw, cachedPrice),
                volume24h: raw.volume24h,
                signer: raw.signer,
                signature: raw.signature,
            };

            this.pendingPrices.set(asset, { price: raw.price, timestamp: raw.timestamp });

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
     * Commit a pending validated price after the on-chain update succeeds.
     */
    commit(asset: string): void {
        const normalizedAsset = asset.toUpperCase();
        const pending = this.pendingPrices.get(normalizedAsset);

        if (pending === undefined) {
            logger.warn(`Commit requested without pending price for ${normalizedAsset}`);
            return;
        }

        this.cachedPrices.set(normalizedAsset, {
            price: pending.price,
            timestamp: pending.timestamp,
        });
        this.pendingPrices.delete(normalizedAsset);
    }

    /**
     * Rollback a pending validated price after the on-chain update fails.
     */
    rollback(asset: string): void {
        const normalizedAsset = asset.toUpperCase();
        const didRollback = this.pendingPrices.delete(normalizedAsset);

        if (!didRollback) {
            logger.warn(`Rollback requested without pending price for ${normalizedAsset}`);
        }
    }

    /**
     * Validate multiple prices
     */
    validateMany(prices: RawPriceData[]): ValidationResult[] {
        return prices.map((p) => this.validate(p));
    }

    /**
     * Validate raw price data, falling back to the latest cached price when the
     * live price is rejected exclusively for staleness and a safe cached price
     * exists. This method never relaxes asset bounds, safe scaling, or hard
     * price limits; it only provides a bounded fallback while the live source
     * is stale.
     */
    validateWithFallback(raw: RawPriceData): ValidationResult {
        const result = this.validate(raw);
        if (result.isValid) {
            return result;
        }

        const isStaleOnly =
            result.errors.length > 0 &&
            result.errors.every((error) => error.code === 'PRICE_STALE');
        if (!isStaleOnly) {
            return result;
        }

        const fallback = this.getFallbackPrice(raw.asset);
        if (fallback === undefined) {
            return result;
        }

        return {
            isValid: true,
            price: fallback,
            errors: [],
        };
    }

    /**
     * Check whether a cached price is fresh enough to use as a reference.
     */
    private isFresh(cached: CachedPrice, now: number): boolean {
        const age = now - cached.timestamp;
        return age >= 0 && age <= this.config.maxStalenessSeconds;
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
            case 'fallback':
                confidence -= 25;
                break;
        }

        return Math.max(0, Math.min(100, confidence));
    }

    /**
     * Update cached price manually (e.g., after successful contract update)
     */
    updateCache(asset: string, price: number, timestamp?: number): void {
        const normalizedAsset = asset.toUpperCase();
        const lastTimestamp = this.cachedPrices.get(normalizedAsset)?.timestamp ?? -Infinity;
        const nextTimestamp = timestamp ?? Math.floor(Date.now() / 1000);

        if (nextTimestamp < lastTimestamp) {
            logger.warn(`Ignoring cache update for ${normalizedAsset}: timestamp ${nextTimestamp} older than last accepted ${lastTimestamp}`);
            return;
        }

        this.cachedPrices.set(normalizedAsset, { price, timestamp: nextTimestamp });
        this.pendingPrices.delete(normalizedAsset);
    }

    /**
     * Reload validator settings and optional bounds at runtime
     */
    reloadConfig(
        config: Partial<ValidatorConfig> = {},
        assetBounds?: Record<string, AssetPriceBounds>,
    ): void {
        const nextConfig = { ...this.config, ...config };
        this.validateConfig(nextConfig);
        const nextBounds = assetBounds ? this.normalizeBounds(assetBounds) : undefined;

        this.config = nextConfig;
        if (nextBounds) {
            this.assetBounds = nextBounds;
        }

        logger.info('Price validator configuration reloaded', {
            maxDeviationPercent: this.config.maxDeviationPercent,
            maxStalenessSeconds: this.config.maxStalenessSeconds,
            maxFallbackStalenessSeconds: this.getFallbackStalenessSeconds(),
            boundsUpdated: assetBounds ? Object.keys(assetBounds).length : 0,
        });
    }

    /**
     * Clear cached price for an asset
     */
    clearCache(asset?: string): void {
        if (asset) {
            const normalizedAsset = asset.toUpperCase();
            this.cachedPrices.delete(normalizedAsset);
            this.pendingPrices.delete(normalizedAsset);
        } else {
            this.cachedPrices.clear();
            this.pendingPrices.clear();
        }
    }

    /**
     * Get current cache state (for debugging)
     */
    getCacheState(): Record<string, number> {
        return Object.fromEntries(
            Array.from(this.cachedPrices.entries()).map(([asset, state]) => [asset, state.price]),
        );
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
            Object.entries(bounds).map(([asset, value]) => {
                const normalizedAsset = asset.toUpperCase();
                this.validateBounds(normalizedAsset, value);
                return [
                    normalizedAsset,
                    {
                        minPrice: value.minPrice,
                        maxPrice: value.maxPrice,
                    },
                ] as [string, AssetPriceBounds];
            }),
        );
    }

    private validateConfig(config: ValidatorConfig): void {
        if (!Number.isFinite(config.maxDeviationPercent) || config.maxDeviationPercent <= 0) {
            throw new Error('maxDeviationPercent must be a finite number greater than 0');
        }
        if (!Number.isFinite(config.maxStalenessSeconds) || config.maxStalenessSeconds <= 0) {
            throw new Error('maxStalenessSeconds must be a finite number greater than 0');
        }
        if (
            config.maxFallbackStalenessSeconds !== undefined &&
            (!Number.isFinite(config.maxFallbackStalenessSeconds) ||
                config.maxFallbackStalenessSeconds <= 0)
        ) {
            throw new Error(
                'maxFallbackStalenessSeconds must be a finite number greater than 0',
            );
        }
        if (!Number.isFinite(config.minPrice) || config.minPrice <= 0) {
            throw new Error('minPrice must be a finite number greater than 0');
        }
        if (!Number.isFinite(config.maxPrice) || config.maxPrice < config.minPrice) {
            throw new Error('maxPrice must be a finite number greater than or equal to minPrice');
        }
    }

    private validateBounds(asset: string, bounds: AssetPriceBounds): void {
        if (!Number.isFinite(bounds.minPrice) || bounds.minPrice <= 0) {
            throw new Error(
                `Invalid bounds for ${asset}: minPrice must be a finite number greater than 0`,
            );
        }
        if (!Number.isFinite(bounds.maxPrice) || bounds.maxPrice < bounds.minPrice) {
            throw new Error(
                `Invalid bounds for ${asset}: maxPrice must be a finite number greater than or equal to minPrice`,
            );
        }
    }
}

/**
 * Create a validator with custom configuration
 */
export function createValidator(
    config?: Partial<ValidatorConfig>,
    assetBounds: Record<string, AssetPriceBounds> = {},
    trustedSigners: Record<string, string[]> = {},
    signatureDomain: string = 'StellarLendOracle',
): PriceValidator {
    return new PriceValidator(config, assetBounds, trustedSigners, signatureDomain);
}
