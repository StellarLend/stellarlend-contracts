/**
 * Base Price Provider
 *
 * Abstract base class for all price data providers.
 * Implements common functionality like rate limiting and error handling.
 *
 * Invariants:
 * - Rate limiting is enforced per provider instance; a single instance must not
 *   exceed `maxRequests` within `any windowMs` window.
 * - A cooldown is only entered after a rate-limit response from the upstream;
 *   while cooled down, no new requests are issued.
 * - Invalid inputs (empty asset, non-positive rate limit) are rejected before
 *   any network I/O oraccurs.
 * - Partial failures in batch fetches are isolated: one asset failing must not
 *   prevent other assets from being fetched or corrupt the result array.
 * - Retries are bounded and exponentially backed off; they must not amplify
 *   load or bypass rate limiting.
 */

import axios, { AxiosError } from 'axios';
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
 * Maximum number of attempts for a single request (including the first).
 */
const MAX_REQUEST_ATTEMPTS = 3;

/**
 * Base delay for exponential backoff in milliseconds.
 */
const BASE_BACKOFF_MS = 250;

/**
 * Maximum backoff delay in milliseconds.
 */
const MAX_BACKOFF_MS = 5000;

/**
 * Error thrown when an invalid asset identifier is provided.
 */
export class InvalidAssetError extends Error {
    constructor(asset: unknown) {
        super(`Invalid asset identifier: ${String(asset)}`);
        this.name = 'InvalidAssetError';
    }
}

/**
 * Error thrown when the provider is cooled down due to an upstream rate limit.
 */
export class ProviderCooldownError extends Error {
    constructor(readonly provider: string, readonly cooldownUntil: number) {
        super(`Provider ${provider} is cooled down until ${cooldownUntil}`);
        this.name = 'ProviderCooldownError';
    }
}

/**
 * Error thrown when a response fails validation (empty body, missing fields,
 * non-positive price, etc.)
 */
export class ProviderResponseError extends Error {
    constructor(readonly provider: string, readonly asset: string, readonly reason: string) {
        super(`Invalid response from ${provider} for ${asset}: ${reason}`);
        this.name = 'ProviderResponseError';
    }
}

/**
 * Abstract base class for price providers
 */
export abstract class BasePriceProvider {
    protected config: ProviderConfig;
    protected lastRequestTime: number = 0;
    protected requestCount: number = 0;
    protected windowStartTime: number = Date.now();
    private rateLimitQueue: Promise<void> = Promise.resolve();
    public cooldownUntil: number = 0;

    /**
     * Serializes rate-limit admission and cooldown checks so concurrent callers
     * cannot bypass the limit or observe a partially-updated window.
     */
    private rateLimitChain: Promise<void> = Promise.resolve();

    constructor(config: ProviderConfig) {
        if (!config || typeof config !== 'object') {
            throw new TypeError('ProviderConfig is required');
        }
        if (!config.name || typeof config.name !== 'string') {
            throw new TypeError('ProviderConfig.name must be a non-empty string');
        }
        const rateLimit = config.rateLimit;
        if (!rateLimit || typeof rateLimit !== 'object') {
            throw new TypeError('ProviderConfig.rateLimit is required');
        }
        if (!Number.isFinite(rateLimit.maxRequests) || rateLimit.maxRequests <= 0) {
            throw new RangeError('rateLimit.maxRequests must be a positive integer');
        }
        if (!Number.isFinite(rateLimit.windowMs) || rateLimit.windowMs <= 0) {
            throw new RangeError('rateLimit.windowMs must be a positive number');
        }
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
     *
     * Partial failures are isolated: a failure for one asset does not affect
     * the others. Duplicate assets are deduplicated (case-insensitively) to
     * avoid wasting rate-limit budget and to keep the result deterministic.
     */
    async fetchPrices(assets: string[]): Promise<RawPriceData[]> {
        if (!Array.isArray(assets)) {
            throw new TypeError('assets must be an array');
        }

        const seen = new Set<string>();
        const normalized: string[] = [];
        for (const asset of assets) {
            const norm = this.normalizeAsset(asset);
            if (!seen.has(norm)) {
                seen.add(norm);
                normalized.push(norm);
            }
        }

        const results: RawPriceData[] = [];

        for (const asset of normalized) {
            try {
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
     * Enforce rate limiting.
     *
     * Admission is serialized through `rateLimitChain` so concurrent callers
     * cannot bypass the limit. While a cooldown is active, the caller is rejected
     * with `ProviderCooldownError` instead of blocking the event loop.
     */
    protected async enforceRateLimit(): Promise<void> {
        const previous = this.rateLimitChain;
        let release!: () => void;
        this.rateLimitChain = new Promise<void>((resolve) => {
            release = resolve;
        });

        await previous;
        try {
            if (this.isCooledDown) {
                throw new ProviderCooldownError(this.name, this.cooldownUntil);
            }

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
        } finally {
            release();
        }
    }

    /**
     * Sleep util
     */
    protected sleep(ms: number): Promise<void> {
        return new Promise((resolve) => setTimeout(resolve, Math.max(0, ms)));
    }

    /**
     * Normalize and validate an asset identifier.
     * Throws `InvalidAssetError` for empty, non-string, or overly long inputs.
     */
    protected normalizeAsset(asset: unknown): string {
        if (typeof asset !== 'string') {
            throw new InvalidAssetError(asset);
        }
        const trimmed = asset.trim().toUpperCase();
        if (trimmed.length === 0 || trimmed.length > 32) {
            throw new InvalidAssetError(asset);
        }
        // Asset identifiers must be alphanumeric with optional separators.
        if (!/^[A-Z0-9._:-]+$/.test(trimmed)) {
            throw new InvalidAssetError(asset);
        }
        return trimmed;
    }

    /**
     * Validate a raw price response from a provider.
     * Ensures the price is a positive finite number and the asset matches.
     */
    protected validateRawPrice(asset: string, data: RawPriceData): RawPriceData {
        if (!data || typeof data !== 'object') {
            throw new ProviderResponseError(this.name, asset, 'empty response');
        }
        const price = data.price;
        if (typeof price !== 'number' || !Number.isFinite(price) || price <= 0) {
            throw new ProviderResponseError(this.name, asset, 'price must be a positive finite number');
        }
        if (data.asset && typeof data.asset === 'string' && data.asset.toUpperCase() !== asset) {
            throw new ProviderResponseError(this.name, asset, 'asset mismatch in response');
        }
        return data;
    }

    /**
     * Make HTTP request using axios with IPv4 forced.
     *
     * Retries on transient failures with exponential backoff. Rate-limit
     * responses (503 or 429 with retry-after) set a cooldown and are not
     * retried immediately.
     */
    protected async request<T>(
        url: string,
        options: { headers?: Record<string, string> } = {},
    ): Promise<T> {
        if (typeof url !== 'string' || url.length === 0) {
            throw new TypeError('request url must be a non-empty string');
        }

        let attempt = 0;
        // eslint-disable-next no-constant-condition
        while (true) {
            attempt++;
            try {
                const response = await axios.get<T>(url, {
                    headers: {
                        'Content-Type': 'application/json',
                        ...options.headers,
                    },
                    timeout: 30000,
                    httpsAgent,
                });

                return response.data;
            } catch (error) {
                const isRetryable = this.isRetryableError(error);
                if (!isRetryable || attempt >= MAX_REQUEST_ATTEMPTS) {
                    throw error;
                }
                const delay = Math.min(BASE_BACKOFF_MS * 2 ** (attempt - 1), MAX_BACKOFF_MS);
                logger.warn(`Retrying ${this.name} request (attempt ${attempt}/${MAX_REQUEST_ATTEMPTS}) after ${delay}ms`);
                await this.sleep(delay);
            }
        }
    }

    /**
     * Determine whether an error is worth retrying.
     */
    private isRetryableError(error: unknown): boolean {
        if (axios.isAxiosError(error)) {
            const axiosError = error as AxiosError;
            if (!axiosError.response) {
                // Network errors and timeouts are retryable.
                return true;
            }
            const status = axiosError.response.status;
            if (status === 429 || status === 503) {
                this.applyRateLimitCooldown(axiosError);
                return false;
            }
            return status >= 500 && status < 600;
        }
        return false;
    }

    /**
     * Apply a cooldown based on a rate-limit response.
     */
    private applyRateLimitCooldown(error: AxiosError): void {
        const retryAfter = error.response?.headers?.['retry-after'];
        let cooldownMs = 60_000;
        if (typeof retryAfter === 'string') {
            const parsed = Number(retryAfter);
            if (Number.isFinite(parsed) && parsed > 0) {
                cooldownMs = Math.min(parsed * 1000, 300_000);
            }
        }
        this.cooldownUntil = Date.now() + cooldownMs;
        logger.warn(`Provider ${this.name} entered cooldown for ${cooldownMs}ms`);
    }
}
