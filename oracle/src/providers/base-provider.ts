/**
 * Base Price Provider
 * 
 * Abstract base class for all price data providers.
 * Implements common functionality like rate limiting and error handling.
 */

import axios from 'axios';
import https from 'https';
import type { RawPriceData, ProviderConfig, HealthStatus } from '../types/index.js';
import { logger } from '../utils/logger.js';

/**
 * HTTPS Agent
 */
const DEFAULT_REQUEST_TIMEOUT_MS = 30000;

const httpsAgent = new https.Agent({
    family: 4,
    keepAlive: true,
    timeout: 30000,
});

/**
 * Abstract base class for price providers
 */
const MAX_ASSET_LENGTH = 32;

export abstract class BasePriceProvider {
    protected config: ProviderConfig;
    protected lastRequestTime: number = 0;
    protected requestCount: number = 0;
    protected windowStartTime: number = Date.now();
    public cooldownUntil: number = 0;

    private rateLimitChain: Promise<void> = Promise.resolve();

    constructor(config: ProviderConfig) {
        this.config = config;
    }

    /**
     * Check if the provider is currently in a rate-limit cooldown
     */
    get isCooledDown(): boolean {
        return this.cooldownUntil > Date.now();
    }

    /**
     * Get provider name
     */
    get name(): string {
        if (!this.config || typeof this.config.name !== 'string' || this.config.name.length === 0) {
            return 'unknown';
        }

        return this.config.name;
    }

    /**
     * Get provider priority
     */
    get priority(): number {
        if (!this.config || !Number.isFinite(this.config.priority)) {
            return Number.MAX_SAFE_INTEGER;
        }

        return this.config.priority;
    }

    /**
     * Get the provider weight for aggregation
     */
    get weight(): number {
        if (!this.config || !Number.isFinite(this.config.weight) || this.config.weight < 0) {
            return 0;
        }

        return this.config.weight;
    }

    /**
     * Check if the provider is enabled
     */
    get isEnabled(): boolean {
        if (!this.config || typeof this.config.enabled !== 'boolean') {
            return false;
        }

        return this.config.enabled;
    }

    /**
     * Fetch price for a specific asset
     * Must be implemented by each provider
     */
    abstract fetchPrice(asset: string): Promise<RawPriceData>;

    /**
     * Fetch prices for multiple assets
     * Can be overridden for batch API calls
     */
    async fetchPrices(assets: string[]): Promise<RawPriceData[]> {
        if (!Array.isArray(assets)) {
            logger.warn(`Invalid assets argument for ${this.name}, expected array`);
            return [];
        }

        const results: RawPriceData[] = [];
        const seen = new Set<string>();

        for (const asset of assets) {
            if (typeof asset !== 'string' || asset.length === 0 || asset.length > MAX_ASSET_LENGTH) {
                logger.warn(`Skipping invalid asset for ${this.name}`, { asset });
                continue;
            }

            const normalized = asset.toUpperCase();
            if (seen.has(normalized)) {
                logger.warn(`Skipping duplicate asset for ${this.name}`, { asset: normalized });
                continue;
            }
            seen.add(normalized);

            try {
                await this.enforceRateLimit();
                const price = await this.fetchPrice(normalized);
                if (!price || typeof price !== 'object') {
                    logger.error(`Provider ${this.name} returned invalid price for ${normalized}`);
                    continue;
                }
                results.push(price);
            } catch (error) {
                logger.error(`Failed to fetch ${normalized} from ${this.name}`, {
                    error: error instanceof Error ? error.message : String(error),
                });
            }
        }

        return results;
    }

    /**
     * Check provider health
     */
    async healthCheck(): Promise<HealthStatus> {
        if (!this.isEnabled) {
            return {
                provider: this.name,
                healthy: false,
                lastCheck: Date.now(),
                latencyMs: 0,
                error: 'Provider is disabled',
            };
        }

        const startTime = Date.now();

        try {
            await this.fetchPrice('XLM');

            return {
                provider: this.name,
                healthy: true,
                lastCheck: Date.now(),
                latencyMs: Date.now() - startTime,
            };
        } catch (error) {
            return {
                provider: this.name,
                healthy: false,
                lastCheck: Date.now(),
                latencyMs: Date.now() - startTime,
                error: error instanceof Error ? error.message : 'Unknown error',
            };
        }
    }

    /**
     * Enforce rate limiting
     */
    protected async enforceRateLimit(): Promise<void> {
        const previous = this.rateLimitChain;
        let release: () => void = () => undefined;
        this.rateLimitChain = new Promise<void>((resolve) => {
            release = resolve;
        });

        await previous;

        try {
            const rateLimit = this.config && this.config.rateLimit;
            const maxRequests = rateLimit && Number.isFinite(rateLimit.maxRequests) && rateLimit.maxRequests > 0
                ? Math.floor(rateLimit.maxRequests)
                : 1;
            const windowMs = rateLimit && Number.isFinite(rateLimit.windowMs) && rateLimit.windowMs > 0
                ? Math.floor(rateLimit.windowMs)
                : 1000;

            let now = Date.now();

            if (now - this.windowStartTime >= windowMs) {
                this.windowStartTime = now;
                this.requestCount = 0;
            }

            if (this.requestCount >= maxRequests) {
                const waitTime = Math.max(0, windowMs - (now - this.windowStartTime));
                logger.warn(`Rate limit reached for ${this.name}, waiting ${waitTime}ms`);
                await this.sleep(waitTime);
                now = Date.now();
                this.windowStartTime = now;
                this.requestCount = 0;
            }

            this.requestCount++;
            this.lastRequestTime = now;
        } finally {
            release();
        }
    }

    /**
     * Sleep util
     */
    protected sleep(ms: number): Promise<void> {
        const delay = Number.isFinite(ms) && ms > 0 ? Math.floor(ms) : 0;
        return new Promise((resolve) => setTimeout(resolve, delay));
    }

    /**
     * Make HTTP request using axios with IPv4 forced
     */
    protected async request<T>(
        url: string,
        options: { headers?: Record<string, string> } = {},
    ): Promise<T> {
        if (typeof url !== 'string' || url.length === 0) {
            throw new Error(`Invalid request URL for provider ${this.name}`);
        }

        const response = await axios.get<T>(url, {
            headers: {
                'Content-Type': 'application/json',
                ...options.headers,
            },
            timeout: DEFAULT_REQUEST_TIMEOUT_MS,
            httpsAgent,
        });

        return response.data;
    }
}
