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

        const now = Math.floor(Date.now() / 1000);
        const fallbackAge = now - fallback.timestamp;
        const maxFallbackStaleness = this.getFallbackStalenessSeconds();

        if (fallbackAge > maxFallbackStaleness) {
            logger.warn(`Cached fallback price for ${raw.asset} is too stale`, {
                fallbackAge,
                maxFallbackStaleness,
            });
            return result;
        }

        const asset = raw.asset.toUpperCase();
        const bounds = this.getBounds(asset);
        if (fallback.price < bounds.minPrice || fallback.price > bounds.maxPrice) {
            logger.warn(`Cached fallback price for ${asset} is outside configured bounds`, {
                fallbackPrice: fallback.price,
                minPrice: bounds.minPrice,
                maxPrice: bounds.maxPrice,
            });
            return result;
        }

        const scaledFallbackPrice = scalePrice(fallback.price);
        if (!Number.isSafeInteger(scaledFallbackPrice)) {
            logger.warn(`Scaled cached fallback price for ${asset} is not a safe integer`, {
                scaledFallbackPrice,
            });
            return result;
        }

        const validatedPrice: PriceData = {
            asset,
            price: scaledFallbackPrice,
            timestamp: fallback.timestamp,
            source: `cached:${raw.source}`,
            confidence: this.calculateFallbackConfidence(fallbackAge, maxFallbackStaleness),
            volume24h: fallback.volume24h,
            signer: raw.signer,
            signature: raw.signature,
        };

        logger.info(`Using cached fallback price for ${asset}`, {
            fallbackAge,
            fallbackPrice: fallback.price,
        });

        return {
            isValid: true,
            price: validatedPrice,
            errors: [],
        };
    }

    /**
     * Return the latest cached price for an asset, if any.
     */
    getCachedPrice(asset: string): CachedPrice | undefined {
        return this.cachedPrices.get(asset.toUpperCase());
    }

    /**
     * Return the pending price for an asset, if any.
     */
    getPendingPrice(asset: string): PendingPrice | undefined {
        return this.pendingPrices.get(asset.toUpperCase());
    }

    /**
     * Clear all cached and pending prices. Primarily for testing and
     * operational recovery.
     */
    reset(): void {
        this.cachedPrices.clear();
        this.pendingPrices.clear();
    }

    /**
     * Get the configured bounds for an asset, falling back to global limits.
     */
    getBounds(asset: string): AssetPriceBounds {
        const normalizedAsset = asset.toUpperCase();
        const configured = this.assetBounds[normalizedAsset];
        if (configured !== undefined) {
            return configured;
        }
        return {
            minPrice: this.config.minPrice,
            maxPrice: this.config.maxPrice,
        };
    }

    /**
     * Return the configured max fallback staleness in seconds.
     */
    getFallbackStalenessSeconds(): number {
        return this.config.maxFallbackStalenessSeconds ?? this.config.maxStalenessSeconds * 3;
    }

    /**
     * Return the current configuration (copy).
     */
    getConfig(): ValidatorConfig {
        return { ...this.config };
    }

    /**
     * Return the configured trusted signers for an asset.
     */
    getTrustedSigners(asset: string): string[] {
        return this.trustedSigners[asset.toUpperCase()] ?? [];
    }

    /**
     * Return the configured signature domain.
     */
    getSignatureDomain(): string {
        return this.signatureDomain;
    }

    /**
     * Normalize asset bounds keys to uppercase and validate bound values.
     */
    private normalizeBounds(input: Record<string, AssetPriceBounds>): Record<string, AssetPriceBounds> {
        const output: Record<string, AssetPriceBounds> = {};
        for (const [key, bounds] of Object.entries(input)) {
            if (!bounds || !Number.isFinite(bounds.minPrice) || !Number.isFinite(bounds.maxPrice)) {
                throw new Error(`Invalid price bounds for asset ${key}`);
            }
            if (bounds.minPrice <= 0 || bounds.maxPrice < bounds.minPrice) {
                throw new Error(`Invalid price bounds range for asset ${key}`);
            }
            output[key.toUpperCase()] = {
                minPrice: bounds.minPrice,
                maxPrice: bounds.maxPrice,
            };
        }
        return output;
    }

    /**
     * Validate the validator configuration.
     */
    private validateConfig(config: ValidatorConfig): void {
        if (!Number.isFinite(config.maxDeviationPercent) || config.maxDeviationPercent < 0) {
            throw new Error('maxDeviationPercent must be a non-negative finite number');
        }
        if (!Number.isFinite(config.maxStalenessSeconds) || config.maxStalenessSeconds < 0) {
            throw new Error('maxStalenessSeconds must be a non-negative finite number');
        }
        if (!Number.isFinite(config.minPrice) || config.minPrice <= 0) {
            throw new Error('minPrice must be a positive finite number');
        }
        if (!Number.isFinite(config.maxPrice) || config.maxPrice < config.minPrice) {
            throw new Error('maxPrice must be a finite number greater than or equal to minPrice');
        }
        if (
            config.maxFallbackStalenessSeconds !== undefined &&
            (!Number.isFinite(config.maxFallbackStalenessSeconds) ||
                config.maxFallbackStalenessSeconds < 0)
        ) {
            throw new Error('maxFallbackStalenessSeconds must be a non-negative finite number');
        }
    }

    /**
     * Calculate a confidence score for a validated price.
     */
    private calculateConfidence(raw: RawPriceData, cachedPrice?: number): number {
        let confidence = 1;

        const now = Math.floor(Date.now() / 1000);
        const age = Math.max(0, now - raw.timestamp);
        const maxAge = this.config.maxStalenessSeconds;
        if (maxAge > 0) {
            confidence *= 1 - Math.min(1, age / maxAge) * 0.5;
        }

        if (cachedPrice !== undefined && cachedPrice > 0) {
            const deviation = Math.abs((raw.price - cachedPrice) / cachedPrice) * 100;
            const maxDeviation = this.config.maxDeviationPercent;
            if (maxDeviation > 0) {
                confidence *= 1 - Math.min(1, deviation / maxDeviation) * 0.3;
            }
        }

        return Math.max(0, min(1, confidence));
    }

    /**
     * Calculate confidence for a cached fallback price.
     */
    private calculateFallbackConfidence(fallbackAge: number, maxFallbackStaleness: number): number {
        if (maxFallbackStaleness <= 0) {
            return 0.5;
        }
        const ratio = Math.min(1, Math.max(0, fallbackAge / maxFallbackStaleness));
        return Math.max(0.1, 0.5 * (1 - ratio));
    }

    /**
     * Get the latest cached price for an asset if it is within the allowed
     * fallback staleness window.
     */
    private getFallbackPrice(asset: string): CachedPrice | undefined {
        const cached = this.cachedPrices.get(asset.toUpperCase());
        if (cached === undefined) {
            return undefined;
        }
        return cached;
    }
}
