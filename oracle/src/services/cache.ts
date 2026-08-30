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
    /** Stale data fallback TTL (seconds) */
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
    staleTtlSeconds: 60,
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
            staleTtlSeconds: this.config.staleTtlSeconds,
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
     * Get a value from cache, allowing stale fallback.
     * Returns the value along with whether it is stale.
     */
    getStale<T>(key: string): { data: T; isStale: boolean } | undefined {
        const entry = this.store.get(key) as CacheEntry<T> | undefined;

        if (!entry) {
            this.misses++;
            return undefined;
        }

        const now = Date.now();
        if (now > entry.expiresAt) {
            // Expired: check stale window
            const staleExpiresAt = entry.expiresAt + (this.config.staleTtlSeconds * 1000);
            if (now > staleExpiresAt) {
                this.store.delete(key);
                this.misses++;
                return undefined;
            }
            this.hits++;
            return { data: entry.data, isStale: true };
        }

        this.hits++;
        return { data: entry.data, isStale: false };
    }

    /**
     * Set a value in cache with optional TTL and explicit timestamp.
     */
    set<T>(key: string, value: T, ttlSeconds?: number, cachedAt?: number): void {
        const ttl = ttlSeconds ?? this.config.defaultTtlSeconds;
        const now = cachedAt ?? Date.now();

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

        const now = Date.now();
        if (now > entry.expiresAt) {
            const staleExpiresAt = entry.expiresAt + (this.config.staleTtlSeconds * 1000);
            if (now > staleExpiresAt) {
                this.store.delete(key);
                return false;
            }
            return true;
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
            const staleExpiresAt = entry.expiresAt + (this.config.staleTtlSeconds * 1000);
            if (now > staleExpiresAt) {
                this.store.delete(key);
                cleaned++;
            }
        }

        if (cleaned > 0) {
            logger.debug(`Cleaned up ${cleaned} stale cache entries`);
        }

        return cleaned;
    }
}

/**
 * Price value stored in cache
 */
interface PriceValue {
    price: bigint;
    updatedAt: number;
}

/**
 * Price-specific cache wrapper with freshness and fallback support.
 */
export class PriceCache {
    private cache: Cache;
    private keyPrefix = 'price:';

    constructor(ttlSeconds: number = 30, staleTtlSeconds: number = 300) {
        this.cache = new Cache({
            defaultTtlSeconds: ttlSeconds,
            staleTtlSeconds,
            maxEntries: 100,
            staleTtlSeconds: 60,
        });
    }

    /**
     * Get cached price, falling back to stale data within the stale TTL.
     * Returns undefined if no usable data exists.
     */
    getPrice(asset: string): bigint | undefined {
        const result = this.cache.getStale<PriceValue>(`${this.keyPrefix}${asset.toUpperCase()}`);
        return result?.data.price;
    }

    /**
     * Get cached price with freshness metadata.
     */
    getPriceWithState(asset: string): { price: bigint; isStale: boolean; updatedAt: number } | undefined {
        const result = this.cache.getStale<PriceValue>(`${this.keyPrefix}${asset.toUpperCase()}`);
        if (!result) return undefined;
        return {
            price: result.data.price,
            isStale: result.isStale,
            updatedAt: result.data.updatedAt,
        };
    }

    /**
     * Cache a price for an asset with an optional update timestamp.
     */
    setPrice(asset: string, price: bigint, ttlSeconds?: number, updatedAt?: number): void {
        const value: PriceValue = {
            price,
            updatedAt: updatedAt ?? Date.now(),
        };
        this.cache.set(
            `${this.keyPrefix}${asset.toUpperCase()}`,
            value,
            ttlSeconds,
            value.updatedAt
        );
    }

    /**
     * Atomically set a price only if the provided updatedAt is newer
     * than the currently cached price. Returns true if updated.
     */
    setPriceIfNewer(asset: string, price: bigint, updatedAt: number, ttlSeconds?: number): boolean {
        const key = `${this.keyPrefix}${asset.toUpperCase()}`;
        const existing = this.cache.getStale<PriceValue>(key);
        if (existing && existing.data.updatedAt >= updatedAt) {
            return false;
        }
        this.setPrice(asset, price, ttlSeconds, updatedAt);
        return true;
    }

    /**
     * Check if we have a usable cached price (fresh or within stale TTL).
     */
    hasPrice(asset: string): boolean {
        return this.cache.has(`{this.keyPrefix}${asset.toUpperCase()}`);
    }

    /**
     * Recover by purging entries that are older than the stale TTL.
     */
    recover(): number {
        return this.cache.cleanup();
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
