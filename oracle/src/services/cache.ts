/**
 * Cache Service
 * 
 * In-memory caching layer with TTL support.
 * Supports Redis too.
 */

import type { CacheEntry } from '../types/index.js';
import { logger } from '../utils/logger.js';

/**
 * Cache config
 */
export interface CacheConfig {
    defaultTtlSeconds: number;
    maxEntries: number;
    /** Redis URL (optional) */
    redisUrl?: string;
}

/**
 * Default cache configuration
 */
const DEFAULT_CONFIG: CacheConfig = {
    defaultTtlSeconds: 30,
    maxEntries: 1000,
};

/**
 * In-memory cache implementation
 */
export class Cache {
    private config: CacheConfig;
    private store: Map<string, CacheEntry<unknown>> = new Map();
    private hits: number = 0;
    private misses: number = 0;

    constructor(config: Partial<CacheConfig> = {}) {
        this.config = { ...DEFAULT_CONFIG, ...config };

        logger.info('Cache initialized', {
            defaultTtlSeconds: this.config.defaultTtlSeconds,
            maxEntries: this.config.maxEntries,
        });
    }

    /**
     * Get a value from cache
     */
    get<T>(key: string): T | undefined {
        const entry = this.store.get(key) as CacheEntry<T> | undefined;

        if (!entry) {
            this.misses++;
            return undefined;
        }

        // Check if expired
        if (Date.now() > entry.expiresAt) {
            this.store.delete(key);
            this.misses++;
            return undefined;
        }

        this.hits++;
        return entry.data;
    }

    /**
     * Set a value in cache with optional TTL
     */
    set<T>(key: string, value: T, ttlSeconds?: number): void {
        const ttl = ttlSeconds ?? this.config.defaultTtlSeconds;
        const now = Date.now();

        // Evict oldest entries if at capacity
        if (this.store.size >= this.config.maxEntries) {
            this.evictOldest();
        }

        const entry: CacheEntry<T> = {
            data: value,
            cachedAt: now,
            expiresAt: now + (ttl * 1000),
        };

        this.store.set(key, entry);
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
     * Check if key exists and is not expired
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
     * Clean up expired entries periodicaly
     */
    cleanup(): number {
        const now = Date.now();
        let cleaned = 0;

        for (const [key, entry] of this.store) {
            if (now > entry.expiresAt) {
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

    constructor(ttlSeconds: number = 30) {
        this.cache = new Cache({
            defaultTtlSeconds: ttlSeconds,
            maxEntries: 100,
        });
    }

    /**
     * Get cached price for an asset
     */
    getPrice(asset: string): bigint | undefined {
        return this.cache.get<bigint>(`${this.keyPrefix}${asset.toUpperCase()}`);
    }

    /**
     * Cache a price for an asset
     */
    setPrice(asset: string, price: bigint, ttlSeconds?: number): void {
        this.cache.set(`${this.keyPrefix}${asset.toUpperCase()}`, price, ttlSeconds);
    }

    /**
     * Check if we have a cached price
     */
    hasPrice(asset: string): boolean {
        return this.cache.has(`${this.keyPrefix}${asset.toUpperCase()}`);
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
export function createPriceCache(ttlSeconds?: number): PriceCache {
    return new PriceCache(ttlSeconds);
}
