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
const httpsAgent = new https.Agent({
    family: 4,
    keepAlive: true,
    timeout: 30000,
});

/**
 * Abstract base class for price providers
 */
export abstract class BasePriceProvider {
    protected config: ProviderConfig;
    protected lastRequestTime: number = 0;
    protected requestCount: number = 0;
    protected windowStartTime: number = Date.now();

    constructor(config: ProviderConfig) {
        this.config = config;
    }

    /**
     * Get provider name
     */
    get name(): string {
        return this.config.name;
    }

    /**
     * Get provider priority
     */
    get priority(): number {
        return this.config.priority;
    }

    /**
     * Get the provider weight for aggregation
     */
    get weight(): number {
        return this.config.weight;
    }

    /**
     * Check if the provider is enabled
     */
    get isEnabled(): boolean {
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
        const results: RawPriceData[] = [];

        for (const asset of assets) {
            try {
                await this.enforceRateLimit();
                const price = await this.fetchPrice(asset);
                results.push(price);
            } catch (error) {
                logger.error(`Failed to fetch ${asset} from ${this.name}`, { error });
            }
        }

        return results;
    }

    /**
     * Check provider health
     */
    async healthCheck(): Promise<HealthStatus> {
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
        const now = Date.now();
        const { maxRequests, windowMs } = this.config.rateLimit;

        if (now - this.windowStartTime >= windowMs) {
            this.windowStartTime = now;
            this.requestCount = 0;
        }

        if (this.requestCount >= maxRequests) {
            const waitTime = windowMs - (now - this.windowStartTime);
            logger.warn(`Rate limit reached for ${this.name}, waiting ${waitTime}ms`);
            await this.sleep(waitTime);
            this.windowStartTime = Date.now();
            this.requestCount = 0;
        }

        this.requestCount++;
        this.lastRequestTime = now;
    }

    /**
     * Sleep util
     */
    protected sleep(ms: number): Promise<void> {
        return new Promise((resolve) => setTimeout(resolve, ms));
    }

    /**
     * Make HTTP request using axios with IPv4 forced
     */
    protected async request<T>(
        url: string,
        options: { headers?: Record<string, string> } = {},
    ): Promise<T> {
        const response = await axios.get<T>(url, {
            headers: {
                'Content-Type': 'application/json',
                ...options.headers,
            },
            timeout: 30000,
            httpsAgent,
        });

        return response.data;
    }
}
