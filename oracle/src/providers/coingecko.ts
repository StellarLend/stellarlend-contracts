/**
 * CoinGecko Price Provider
 * 
 * Fallback price source using CoinGecko's API.
 * 
 * Supports:
 * - Free tier (no API key): api.coingecko.com, 10-30 calls/min
 * - Demo tier (CG-* key): api.coingecko.com with x-cg-demo-api-key header
 * - Pro tier (other key): pro-api.coingecko.com with x-cg-pro-api-key header
 * 
 * @see https://docs.coingecko.com/reference/simple-price
 */

import { BasePriceProvider } from './base-provider.js';
import type { RawPriceData, ProviderConfig } from '../types/index.js';
import { logger } from '../utils/logger.js';

/**
 * Asset to CoinGecko ID mapping
 */
const COINGECKO_ID_MAP: Record<string, string> = {
    XLM: 'stellar',
    USDC: 'usd-coin',
    USDT: 'tether',
    BTC: 'bitcoin',
    ETH: 'ethereum',
    SOL: 'solana',
    AVAX: 'avalanche-2',
    DOT: 'polkadot',
    MATIC: 'matic-network',
    LINK: 'chainlink',
};

/**
 * CoinGecko API response for simple price endpoint
 */
interface CoinGeckoSimplePriceResponse {
    [coinId: string]: {
        usd: number;
        usd_24h_change?: number;
        last_updated_at?: number;
    };
}

/**
 * Determine API tier from API key
 * - No key: Free tier
 * - Key starting with CG-: Demo tier
 * - Other key: Pro tier
 */
function getApiTier(apiKey?: string): 'free' | 'demo' | 'pro' {
    if (!apiKey) return 'free';
    if (apiKey.startsWith('CG-')) return 'demo';
    return 'pro';
}

/**
 * CoinGecko Price Provider
 */
export class CoinGeckoProvider extends BasePriceProvider {
    private apiKey?: string;
    private tier: 'free' | 'demo' | 'pro';

    constructor(config: ProviderConfig) {
        super(config);
        this.apiKey = config.apiKey;
        this.tier = getApiTier(config.apiKey);

        logger.info('CoinGecko provider initialized', {
            tier: this.tier,
            baseUrl: config.baseUrl,
        });
    }

    /**
     * Get the correct header name for the API key
     */
    private getApiKeyHeader(): string {
        return this.tier === 'pro' ? 'x-cg-pro-api-key' : 'x-cg-demo-api-key';
    }

    /**
     * Map asset symbol to CoinGecko ID
     */
    private getCoingeckoId(asset: string): string {
        const id = COINGECKO_ID_MAP[asset.toUpperCase()];
        if (!id) {
            throw new Error(`Asset ${asset} not mapped for CoinGecko`);
        }
        return id;
    }

    /**
     * Fetch price for a specific asset
     */
    async fetchPrice(asset: string): Promise<RawPriceData> {
        if (this.isCooledDown) {
            throw new Error(`CoinGecko provider is in cooldown until ${new Date(this.cooldownUntil).toISOString()}`);
        }

        const coinId = this.getCoingeckoId(asset);

        await this.enforceRateLimit();

        const url = `${this.config.baseUrl}/simple/price?ids=${coinId}&vs_currencies=usd&include_last_updated_at=true`;

        const headers: Record<string, string> = {};
        if (this.apiKey) {
            headers[this.getApiKeyHeader()] = this.apiKey;
        }

        try {
            const response = await this.request<CoinGeckoSimplePriceResponse>(url, { headers });

            const coinData = response[coinId];
            if (!coinData) {
                throw new Error(`No price data returned for ${coinId}`);
            }

            return {
                asset: asset.toUpperCase(),
                price: coinData.usd,
                timestamp: coinData.last_updated_at || Math.floor(Date.now() / 1000),
                source: 'coingecko',
            };
        } catch (error) {
            this.handleRateLimitError(error);
            logger.error(`CoinGecko fetch failed for ${asset}`, { error });
            throw error;
        }
    }

    /**
     * Fetch prices for multiple assets (batch API call)
     */
    async fetchPrices(assets: string[]): Promise<RawPriceData[]> {
        if (this.isCooledDown) {
            throw new Error(`CoinGecko provider is in cooldown until ${new Date(this.cooldownUntil).toISOString()}`);
        }

        // Map all assets to CoinGecko IDs
        const assetToId: Map<string, string> = new Map();
        const validAssets: string[] = [];

        for (const asset of assets) {
            try {
                const id = this.getCoingeckoId(asset);
                assetToId.set(asset.toUpperCase(), id);
                validAssets.push(asset.toUpperCase());
            } catch {
                logger.warn(`Skipping unsupported asset: ${asset}`);
            }
        }

        if (validAssets.length === 0) {
            return [];
        }

        await this.enforceRateLimit();

        const coinIds = validAssets.map((a) => assetToId.get(a)!).join(',');
        const url = `${this.config.baseUrl}/simple/price?ids=${coinIds}&vs_currencies=usd&include_last_updated_at=true`;

        const headers: Record<string, string> = {};
        if (this.apiKey) {
            headers[this.getApiKeyHeader()] = this.apiKey;
        }

        try {
            const response = await this.request<CoinGeckoSimplePriceResponse>(url, { headers });

            const results: RawPriceData[] = [];

            for (const asset of validAssets) {
                const coinId = assetToId.get(asset)!;
                const coinData = response[coinId];

                if (coinData) {
                    results.push({
                        asset,
                        price: coinData.usd,
                        timestamp: coinData.last_updated_at || Math.floor(Date.now() / 1000),
                        source: 'coingecko',
                    });
                }
            }

            return results;
        } catch (error) {
            this.handleRateLimitError(error);
            logger.error('CoinGecko batch fetch failed', { error });
            throw error;
        }
    }

    /**
     * Get supported assets
     */
    getSupportedAssets(): string[] {
        return Object.keys(COINGECKO_ID_MAP);
    }

    /**
     * Parses the Retry-After header.
     * Can be a number of seconds or an HTTP-date.
     * Returns delay in milliseconds, or null if parsing fails.
     */
    private parseRetryAfter(headerValue?: string | string[]): number | null {
        if (!headerValue) return null;
        const valueStr = Array.isArray(headerValue) ? headerValue[0] : headerValue;
        if (!valueStr) return null;

        // Check if it's a non-negative integer (seconds)
        const seconds = parseInt(valueStr, 10);
        if (!isNaN(seconds) && /^\d+$/.test(valueStr.trim())) {
            return seconds * 1000;
        }

        // Try parsing as a Date string
        const parsedDate = Date.parse(valueStr);
        if (!isNaN(parsedDate)) {
            const ms = parsedDate - Date.now();
            return ms > 0 ? ms : 0;
        }

        return null;
    }

    /**
     * Inspects error and sets cooldown if 429 rate limited
     */
    private handleRateLimitError(error: any): void {
        if (error && error.response?.status === 429) {
            const retryAfterHeader = error.response?.headers?.['retry-after'];
            const delayMs = this.parseRetryAfter(retryAfterHeader) ?? 60000; // 60s default
            this.cooldownUntil = Date.now() + delayMs;
            logger.warn(`CoinGecko rate limited (429). Suspending provider for ${delayMs}ms (until ${new Date(this.cooldownUntil).toISOString()})`, {
                retryAfter: retryAfterHeader,
            });
        }
    }
}

/**
 * Create a CoinGecko provider with default configuration
 * 
 * API Key Types:
 * - No key: Free tier (api.coingecko.com, 10-30 calls/min)
 * - CG-* key: Demo tier (api.coingecko.com with demo header)
 * - Other key: Pro tier (pro-api.coingecko.com with pro header)
 */
export function createCoinGeckoProvider(apiKey?: string): CoinGeckoProvider {
    const tier = getApiTier(apiKey);

    // Demo and Free use the same base URL, only Pro uses pro-api
    const baseUrl = tier === 'pro'
        ? 'https://pro-api.coingecko.com/api/v3'
        : 'https://api.coingecko.com/api/v3';

    const config: ProviderConfig = {
        name: 'coingecko',
        enabled: true,
        priority: 1,
        weight: 0.6,
        apiKey,
        baseUrl,
        rateLimit: {
            maxRequests: tier === 'free' ? 10 : 500,
            windowMs: 60000,
        },
    };

    return new CoinGeckoProvider(config);
}
