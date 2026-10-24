/**
 * Binance Price Provider
 * 
 * Fallback price source using Binance's public API.
 * No API key required for public market data.
 * 
 * @see https://binance-docs.github.io/apidocs/spot/en/
 */

import { BasePriceProvider } from './base-provider.js';
import type { RawPriceData, ProviderConfig } from '../types/index.js';
import { logger } from '../utils/logger.js';

/**
 * Asset to Binance symbol mapping
 * All pairs are quoted against USDT for USD-equivalent pricing
 */
const BINANCE_SYMBOL_MAP: Record<string, string> = {
    XLM: 'XLMUSDT',
    USDC: 'USDCUSDT',
    BTC: 'BTCUSDT',
    ETH: 'ETHUSDT',
    SOL: 'SOLUSDT',
    AVAX: 'AVAXUSDT',
    DOT: 'DOTUSDT',
    MATIC: 'MATICUSDT',
    LINK: 'LINKUSDT',
    ADA: 'ADAUSDT',
    DOGE: 'DOGEUSDT',
};

/** Maximum allowed age of a ticker before it is considered stale (ms) */
const MAX_TICKER_AGE_MS = 5 * 60 * 1000;

/**
 * Binance 24hr ticker response
 */
interface Binance24hrTickerResponse {
    symbol: string;
    lastPrice: string;
    closeTime: number;
    /** Quote asset volume over the last 24 hours (USD-equivalent for *USDT pairs) */
    quoteVolume: string;
    /** Binance returns this on error responses */
    code?: number;
    msg?: string;
}

/**
 * Binance Price Provider
 */
export class BinanceProvider extends BasePriceProvider {
    constructor(config: ProviderConfig) {
        super(config);

        logger.info('Binance provider initialized', {
            baseUrl: config.baseUrl,
        });
    }

    /**
     * Map asset symbol to Binance trading pair
     */
    private getBinanceSymbol(asset: string): string {
        const symbol = BINANCE_SYMBOL_MAP[asset.toUpperCase()];
        if (!symbol) {
            throw new Error(`Asset ${asset} not mapped for Binance`);
        }
        return symbol;
    }

    /**
     * Parse and validate a ticker response into RawPriceData.
     *
     * Invariants enforced:
     * - lastPrice must be a finite, strictly positive number.
     * - closeTime must be a finite, positive epoch-ms value that is not
     *   unreasonably far in the future and not older than MAX_TICKER_AGE_MS.
     * - quoteVolume, when present, must parse to a non-negative finite number.
     */
    private parseTicker(asset: string, ticker: Binance24hrTickerResponse): RawPriceData {
        if (ticker.code !== undefined) {
            throw new Error(
                `Binance error for ${asset}: ${ticker.msg ?? 'unknown error'} (code ${ticker.code})`,
            );
        }

        const price = Number.parseFloat(ticker.lastPrice);
        if (!Number.isFinite(price) || price <= 0) {
            throw new Error(`Binance returned invalid price for ${asset}: ${ticker.lastPrice}`);
        }

        const closeTime = Number(ticker.closeTime);
        if (!Number.isFinite(closeTime) || closeTime <= 0) {
            throw new Error(`Binance returned invalid closeTime for ${asset}: ${ticker.closeTime}`);
        }

        const nowMs = Date.now();
        if (closeTime > nowMs + MAX_TICKER_AGE_MS) {
            throw new Error(`Binance returned future-dated ticker for ${asset}`);
        }
        if (nowMs - closeTime > MAX_TICKER_AGE_MS) {
            throw new Error(`Binance returned stale ticker for ${asset}`);
        }

        const rawVolume = Number.parseFloat(ticker.quoteVolume);
        const volume = Number.isFinite(rawVolume) && rawVolume > 0 ? rawVolume : 0;

        return {
            asset: asset.toUpperCase(),
            price,
            timestamp: Math.floor(closeTime / 1000),
            source: 'binance',
            volume24h: BigInt(Math.round(volume)),
        };
    }

    /**
     * Fetch price for a specific asset
     */
    async fetchPrice(asset: string): Promise<RawPriceData> {
        const symbol = this.getBinanceSymbol(asset);

        await this.enforceRateLimit();

        const url = `${this.config.baseUrl}/ticker/24hr?symbol=${symbol}`;

        try {
            const response = await this.request<Binance24hrTickerResponse>(url);

            return this.parseTicker(asset, response);
        } catch (error) {
            logger.error(`Binance fetch failed for ${asset}`, { error });
            throw error;
        }
    }

    /**
     * Fetch prices for multiple assets
     * Uses batch ticker endpoint for efficiency
     *
     * Invariants:
     * - Duplicate assets are de-duplicated so each asset appears at most once.
     * - Unsupported assets are skipped (never silently mis-priced).
     * - If the batch response omits an asset, that asset is omitted from the
     *   result rather than returned with a fabricated price.
     */
    async fetchPrices(assets: string[]): Promise<RawPriceData[]> {
        const assetToSymbol: Map<string, string> = new Map();
        const validAssets: string[] = [];

        for (const asset of assets) {
            try {
                const symbol = this.getBinanceSymbol(asset);
                const upper = asset.toUpperCase();
                if (!assetToSymbol.has(upper)) {
                    assetToSymbol.set(upper, symbol);
                    validAssets.push(upper);
                }
            } catch {
                logger.warn(`Skipping unsupported asset: ${asset}`);
            }
        }

        if (validAssets.length === 0) {
            return [];
        }

        await this.enforceRateLimit();

        const symbols = validAssets.map((a) => assetToSymbol.get(a)!);
        const symbolsParam = encodeURIComponent(JSON.stringify(symbols));
        const url = `${this.config.baseUrl}/ticker/24hr?symbols=${symbolsParam}`;

        try {
            const response = await this.request<Binance24hrTickerResponse[]>(url);

            if (!Array.isArray(response)) {
                throw new Error('Binance batch response was not an array');
            }

            // For quick lookup
            const symbolToTicker: Map<string, Binance24hrTickerResponse> = new Map();
            for (const ticker of response) {
                if (ticker && typeof ticker.symbol === 'string') {
                    symbolToTicker.set(ticker.symbol, ticker);
                }
            }

            const results: RawPriceData[] = [];

            for (const asset of validAssets) {
                const symbol = assetToSymbol.get(asset)!;
                const ticker = symbolToTicker.get(symbol);

                if (ticker !== undefined) {
                    try {
                        results.push(this.parseTicker(asset, ticker));
                    } catch (error) {
                        logger.warn(`Skipping invalid ticker for ${asset}`, { error });
                    }
                }
            }

            return results;
        } catch (error) {
            logger.error('Binance batch fetch failed', { error });
            throw error;
        }
    }

    /**
     * Get supported assets
     */
    getSupportedAssets(): string[] {
        return Object.keys(BINANCE_SYMBOL_MAP);
    }
}

/**
 * Create a Binance provider with default configuration
 */
export function createBinanceProvider(): BinanceProvider {
    const config: ProviderConfig = {
        name: 'binance',
        enabled: true,
        priority: 2, // Second priority (after CoinGecko)
        weight: 0.4,
        baseUrl: 'https://api.binance.com/api/v3',
        rateLimit: {
            maxRequests: 1200,
            windowMs: 60000,
        },
    };

    return new BinanceProvider(config);
}
