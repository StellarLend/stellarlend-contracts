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

export interface ValidatorConfig {
    maxDeviationPercent: number;
    maxStalenessSeconds: number;
    minPrice: number;
    maxPrice: number;
    maxFallbackStalenessSeconds?: number;
}

interface CachedPrice {
    price: number;
    timestamp: number;
    volume24h?: number;
}

const DEFAULT_CONFIG: ValidatorConfig = {
    maxDeviationPercent: 10,
    maxStalenessSeconds: 300,
    minPrice: 0.0000001,
    maxPrice: 1000000000,
};

/** Largest scaled price that survives a round trip through a JS number. */
const MAX_SAFE_SCALED_PRICE = BigInt(Number.MAX_SAFE_INTEGER);

/**
 * True when a scaled bigint price can be represented exactly as a JS number.
 *
 * `Number.isSafeInteger` cannot be used here: it only accepts `number`
 * arguments and always returns `false` for a `bigint`, which would reject every
 * price. The bound is compared in bigint arithmetic instead.
 */
function isRepresentableScaledPrice(scaledPrice: bigint): boolean {
    return scaledPrice >= 0n && scaledPrice <= MAX_SAFE_SCALED_PRICE;
}

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
        if (Number.isFinite(raw.price) && !isRepresentableScaledPrice(scaledPrice)) {
            errors.push({
                code: 'PRICE_DEVIATION_TOO_HIGH' as ValidationErrorCode,
                message: `Scaled price ${scaledPrice} for ${asset} is not a safe integer`,
                details: { scaledPrice: scaledPrice.toString(), maxSafeInteger: Number.MAX_SAFE_INTEGER },
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

            // If configured, verify signature for this source
            const trusted = this.trustedSigners[raw.source];
            if (trusted && trusted.length > 0) {
                const sigErr = this.verifySignatureIfPresent(raw, trusted);
                if (sigErr) {
                    return {
                        isValid: false,
                        errors: [sigErr],
                    };
                }
            }

            this.cachedPrices.set(asset, {
                price: raw.price,
                timestamp: raw.timestamp,
                volume24h: raw.volume24h ? Number(raw.volume24h) : undefined,
            });

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
     * Verify provider signature when trusted signers are configured for a source.
     * Returns a ValidationError when verification fails, otherwise undefined.
     */
    private verifySignatureIfPresent(raw: RawPriceData, trusted: string[]): ValidationError | undefined {
        if (!raw.signature || !raw.signer) {
            return {
                code: 'SOURCE_UNAVAILABLE' as ValidationError['code'],
                message: `Missing signature or signer for trusted source ${raw.source}`,
            };
        }

        // Ensure signer is in trusted list
        if (!trusted.includes(raw.signer)) {
            return {
                code: 'SOURCE_UNAVAILABLE' as ValidationError['code'],
                message: `Untrusted signer ${raw.signer} for source ${raw.source}`,
            };
        }

        try {
            const kp = Keypair.fromPublicKey(raw.signer);

            // Canonical message: domain|asset|price|timestamp|source
            const msg = `${this.signatureDomain}|${raw.asset.toUpperCase()}|${raw.price}|${raw.timestamp}|${raw.source}`;
            const msgBuf = Buffer.from(msg, 'utf8');

            const sigBuf = Buffer.from(raw.signature, 'base64');

            const verified = kp.verify(msgBuf, sigBuf);

            if (!verified) {
                return {
                    code: 'SOURCE_UNAVAILABLE' as ValidationError['code'],
                    message: `Invalid signature for source ${raw.source}`,
                };
            }
        } catch (err) {
            logger.error('Signature verification error', { error: err });
            return {
                code: 'SOURCE_UNAVAILABLE' as ValidationError['code'],
                message: `Signature verification failed for ${raw.source}`,
            };
        }

        return undefined;
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
        if (fallbackAge > this.getFallbackStalenessSeconds()) {
            return result;
        }

        const asset = raw.asset.toUpperCase();
        const bounds = this.getBounds(asset);
        if (fallback.price < bounds.minPrice || fallback.price > bounds.maxPrice) {
            return result;
        }

        const scaledFallbackPrice = scalePrice(fallback.price);
        if (!Number.isSafeInteger(scaledFallbackPrice)) {
            return result;
        }

        const validatedPrice: PriceData = {
            asset,
            price: scaledFallbackPrice,
            timestamp: fallback.timestamp,
            source: `${raw.source}:cached`,
            confidence: this.calculateConfidence(raw, fallback.price),
            volume24h: raw.volume24h,
            signer: raw.signer,
            signature: raw.signature,
        };

        this.pendingPrices.set(asset, { price: fallback.price, timestamp: fallback.timestamp });

        return {
            isValid: true,
            price: validatedPrice,
            errors: [],
        };
    }

    /**
     * Get the latest cached price for an asset.
     */
    getCachedPrice(asset: string): CachedPrice | undefined {
        return this.cachedPrices.get(asset.toUpperCase());
    }

    /**
     * Get the pending price for an asset, if any.
     */
    getPendingPrice(asset: string): PendingPrice | undefined {
        return this.pendingPrices.get(asset.toUpperCase());
    }

    /**
     * Clear all cached and pending prices. Useful for testing and recovery.
     */
    reset(): void {
        this.cachedPrices.clear();
        this.pendingPrices.clear();
    }

    private getFallbackStalenessSeconds(): number {
        return this.config.maxFallbackStalenessSeconds ?? 1000000000;
    }

    private getFallbackPrice(asset: string): CachedPrice | undefined {
        return this.cachedPrices.get(asset.toUpperCase());
    }

    private getFallbackStalenessSeconds(): number {
        return this.config.maxFallbackStalenessSeconds ?? this.config.maxStalenessSeconds * 3;
    }

    /**
     * Effective maximum age of a cached price usable as a stale fallback.
     * Falls back to 3x the primary staleness threshold when not configured.
     */
    private getFallbackStalenessSeconds(): number {
        return (
            this.config.maxFallbackStalenessSeconds ?? this.config.maxStalenessSeconds * 3
        );
    }

    private getBounds(asset: string): AssetPriceBounds {
        return (
            this.assetBounds[asset] ?? {
                minPrice: this.config.minPrice,
                maxPrice: this.config.maxPrice,
            }
        );
    }

    private normalizeBounds(
        bounds: Record<string, AssetPriceBounds>,
    ): Record<string, AssetPriceBounds> {
        const normalized: Record<string, AssetPriceBounds> = {};
        for (const [key, value] of Object.entries(bounds)) {
            normalized[key.toUpperCase()] = value;
        }
        return normalized;
    }

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
        if (!Number.isFinite(config.maxPrice) || config.maxPrice <= 0) {
            throw new Error('maxPrice must be a positive finite number');
        }
        if (config.minPrice > config.maxPrice) {
            throw new Error('minPrice must be less than or equal to maxPrice');
        }
        if (
            config.maxFallbackStalenessSeconds !== undefined &&
            (!Number.isFinite(config.maxFallbackStalenessSeconds) ||
                config.maxFallbackStalenessSeconds < 0)
        ) {
            throw new Error('maxFallbackStalenessSeconds must be a non-negative finite number');
        }
    }

    private calculateConfidence(raw: RawPriceData, cachedPrice?: number): number {
        let confidence = 100;
        const now = Math.floor(Date.now() / 1000);
        const age = now - raw.timestamp;
        if (age > 0) {
            confidence -= Math.min(50, Math.floor((age / this.config.maxStalenessSeconds) * 50));
        }
        if (cachedPrice !== undefined && cachedPrice > 0) {
            const deviation = Math.abs((raw.price - cachedPrice) / cachedPrice) * 100;
            confidence -= Math.min(50, Math.floor(deviation * 2));
        }
        return Math.max(0, Math.min(100, confidence));
    }
}
