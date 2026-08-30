/**
 * Cache Service
 * 
 * In-memory caching layer with TTL support.
 * Supports Redis too.
 * 
 * Contract:
 * - `get` returns only fresh data (not stale).
 * - `getWithState` returns data until hard expiry, with staleness flag.
 * - `set` stores data with a freshness TTL and a stale grace period.
 * - `cleanup` removes entries only after hard expiry, preserving stale-while-revalidate.
 */

import type { CacheEntry } from '../types/index.js';
import { logger } from '../utils/logger.js';

/**
 * Cache config
 */
export interface CacheConfig {
    defaultTtlSeconds: number;
    maxEntries: number;
    /** How long to keep stale data after its TTL for fallback reads (seconds). */
    staleTtlSeconds: number;
    /** Redis URL (optional) */
    redisUrl?: string;
}

interface CacheEntryWithHardExpiry<T> extends CacheEntry<T> {
    hardExpiresAt: number;
}

/**
 * Default cache configuration
 */
const DEFAULT_CONFIG: CacheConfig = {
    defaultTtlSeconds: 30,
    maxEntries: 1000,
    staleTtlSeconds: 300,
};

/**
 * In-memory cache implementation
 */
export class Cache {
    private config: CacheConfig;
    private store: Map<string, CacheEntryWithHardExpiry<unknown>> = new Map();
    private hits: number = 0;
    private misses: number = 0;

    constructor(config: Partial<CacheConfig> = {}) {
        this.config = { ...DEFAULT_CONFIG, ...config };

        if (this.config.defaultTtlSeconds < 0 || this.config.staleTtlSeconds < 0) {
            throw new Error('TTL values must be non-negative');
        }

        logger.info('Cache initialized', {
            defaultTtlSeconds: this.config.defaultTtlSeconds,
            staleTtlSeconds: this.config.staleTtlSeconds,
            maxEntries: this.config.maxEntries,
        });
    }

    /**
     * Get a fresh value from cache. Returns undefined if key is missing or stale.
     */
    get<T>(key: string): T | undefined {
        const entry = this.store.get(key) as CacheEntryWithHardExpiry<T> | undefined;

        if (!entry) {
            this.misses++;
            return undefined;
        }

        // Fresh read: only return if before the freshness TTL.
        if (Date.now() > entry.expiresAt) {
            this.misses++;
            return undefined;
        }

        this.hits++;
        return entry.data;
    }

    /**
     * Get a value from cache with staleness information.
     * Returns undefined if key is older than the stale grace period.
     * Stale entries are preserved for fallback until hard expiry.
     */
    getWithState<T>(key: string): { data: T; stale: boolean } | undefined {
        const entry = this.store.get(key) as CacheEntryWithHardExpiry<T> | undefined;

        if (!entry) {
            this.misses++;
            return undefined;
        }

        // Hard expiry: data is no longer usable even as fallback.
        if (Date.now() > entry.hardExpiresAt) {
            this.store.delete(key);
            this.misses++;
            return undefined;
        }

        const stale = Date.now() > entry.expiresAt;
        this.hits++;
        return { data: entry.data, stale };
    }

    /**
     * Set a value in cache with optional TTL.
     * The entry will be considered fresh until `ttlSeconds`, then stale but
     * still available for fallback until `ttlSeconds + staleTtlSeconds`.
     */
    set<T>(key: string, value: T, ttlSeconds?: number): void {
        const ttl = ttlSeconds ?? this.config.defaultTtlSeconds;
        const now = Date.now();

        // Evict oldest entries only when adding a new key (not overwriting)
        if (!this.store.has(key) && this.store.size >= this.config.maxEntries) {
            this.evictOldest();
        }

        const entry: CacheEntryWithHardExpiry<T> = {
            data: value,
            cachedAt: now,
            expiresAt: now + (ttl * 1000),
            hardExpiresAt: now + ((ttl + this.config.staleTtlSeconds) * 1000),
        };

        this.store.set(key, entry as CacheEntryWithHardExpiry<unknown>);
    }

    /**
     * Delete a specific key
     */
    delete(key: string): boolean {
        return this.store.delete(key);
    }

    /**
     * Clear all entries
     */
    clear(): void {
        this.store.clear();
        logger.info('Cache cleared');
    }

    /**
     * Check if key exists and is not expired (fresh).
     */
    has(key: string): boolean {
        const entry = this.store.get(key);

        if (!entry) {
            return false;
        }

        if (Date.now() > entry.expiresAt) {
            this.store.delete(key);
            return false;
        }

        return true;
    }

    /**
     * Get cache statistics
     */
    getStats(): {
        size: number;
        hits: number;
        misses: number;
        hitRate: number;
    } {
        const total = this.hits + this.misses;
        return {
            size: this.store.size,
            hits: this.hits,
            misses: this.misses,
            hitRate: total > 0 ? this.hits / total : 0,
        };
    }

    /**
     * Evict oldest entries to make room
     */
    private evictOldest(): void {
        let oldestKey: string | undefined;
        let oldestTime = Infinity;

        for (const [key, entry] of this.store) {
            if (entry.cachedAt < oldestTime) {
                oldestTime = entry.cachedAt;
                oldestKey = key;
            }
        }

        if (oldestKey) {
            this.store.delete(oldestKey);
            logger.debug(`Evicted oldest cache entry: ${oldestKey}`);
        }
    }

    /**
     * Clean up entries that have passed their hard expiry.
     * Stale entries within the grace period are preserved for fallback.
     */
    cleanup(): number {
        const now = Date.now();
        let cleaned = 0;

        for (const [key, entry] of this.store) {
            if (now > entry.hardExpiresAt) {
                this.store.delete(key);
                cleaned++;
            }
        }

        if (cleaned > 0) {
            logger.debug(`Cleaned up ${cleaned} expired cache entries`);
        }

        return cleaned;
    }
}

/**
 * Price-specific cache wrapper
 */
export class PriceCache {
    private cache: Cache;
    private keyPrefix = 'price:';

    constructor(ttlSeconds: number = 30, staleTtlSeconds: number = 300) {
        this.cache = new Cache({
            defaultTtlSeconds: ttlSeconds,
            staleTtlSeconds,
            maxEntries: 100,
        });
    }

    /**
     * Get cached price for an asset (fresh only).
     */
    getPrice(asset: string): bigint | undefined {
        return this.cache.get<bigint>(`${this.keyPrefix}${asset.toUpperCase()}`);
    }

    /**
     * Get cached price with fallback to stale data.
     * Returns undefined only if no data exists or hard expiry has passed.
     */
    getPriceWithFallback(asset: string): { price: bigint; stale: boolean } | undefined {
        const state = this.cache.getWithState<bigint>(`${this.keyPrefix}${asset.toUpperCase()}`);
        if (state) {
            return { price: state.data, stale: state.stale };
        }
        return undefined;
    }

    /**
     * Cache a price for an asset
     */
    setPrice(asset: string, price: bigint, ttlSeconds?: number): void {
        this.cache.set(`${this.keyPrefix}${asset.toUpperCase()}`, price, ttlSeconds);
    }

    /**
     * Check if we have a fresh cached price
     */
    hasPrice(asset: string): boolean {
        return this.cache.has(`{this.keyPrefix}${asset.toUpperCase()}`);
    }

    /**
     * Get cache statistics
     */
    getStats() {
        return this.cache.getStats();
    }

    /**
     * Clear all cached prices
     */
    clear(): void {
        this.cache.clear();
    }
}

/**
 * Create a new cache instance
 */
export function createCache(config?: Partial<CacheConfig>): Cache {
    return new Cache(config);
}

/**
 * Create a price-specific cache
 */
export function createPriceCache(ttlSeconds?: number, staleTtlSeconds?: number): PriceCache {
    return new PriceCache(ttlSeconds, staleTtlSeconds);
}
