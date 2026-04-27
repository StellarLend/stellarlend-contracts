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

/**
 * Binance ticker price response
 */
interface BinanceTickerResponse {
    symbol: string;
    price: string;
}

/**
 * Binance 24hr ticker response
 */
interface Binance24hrTickerResponse {
    symbol: string;
    lastPrice: string;
    closeTime: number;
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
     * Fetch price for a specific asset
     */
    async fetchPrice(asset: string): Promise<RawPriceData> {
        const symbol = this.getBinanceSymbol(asset);

        await this.enforceRateLimit();

        const url = `${this.config.baseUrl}/ticker/24hr?symbol=${symbol}`;

        try {
            const response = await this.request<Binance24hrTickerResponse>(url);

            return {
                asset: asset.toUpperCase(),
                price: parseFloat(response.lastPrice),
                timestamp: Math.floor(response.closeTime / 1000),
                source: 'binance',
            };
        } catch (error) {
            logger.error(`Binance fetch failed for ${asset}`, { error });
            throw error;
        }
    }

    /**
     * Fetch prices for multiple assets
     * Uses batch ticker endpoint for efficiency
     */
    async fetchPrices(assets: string[]): Promise<RawPriceData[]> {
        const assetToSymbol: Map<string, string> = new Map();
        const validAssets: string[] = [];

        for (const asset of assets) {
            try {
                const symbol = this.getBinanceSymbol(asset);
                assetToSymbol.set(asset.toUpperCase(), symbol);
                validAssets.push(asset.toUpperCase());
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
        const url = `${this.config.baseUrl}/ticker/price?symbols=${symbolsParam}`;

        try {
            const response = await this.request<BinanceTickerResponse[]>(url);

            // For quick lookup
            const symbolToPrice: Map<string, number> = new Map();
            for (const ticker of response) {
                symbolToPrice.set(ticker.symbol, parseFloat(ticker.price));
            }

            const results: RawPriceData[] = [];
            const now = Math.floor(Date.now() / 1000);

            for (const asset of validAssets) {
                const symbol = assetToSymbol.get(asset)!;
                const price = symbolToPrice.get(symbol);

                if (price !== undefined) {
                    results.push({
                        asset,
                        price,
                        timestamp: now,
                        source: 'binance',
                    });
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
