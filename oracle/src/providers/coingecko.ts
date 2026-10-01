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
 * Invariants:
 * - Asset symbols are normalized (trimmed, upper-cased, charset-checked) and
 *   validated before any network I/O or rate-limit budget is consumed, so
 *   invalid input can never reach CoinGecko. Rejections are deterministic and
 *   do not depend on cooldown or rate-limit state.
 * - Every outbound request carries at most one upstream HTTP call per attempt
 *   (retry policy lives in `BasePriceProvider.request`).
 * - A response entry is only turned into `RawPriceData` when it carries a
 *   finite, strictly positive USD price and a sane `last_updated_at`. A missing
 *   or malformed entry is never converted into a fabricated or zero price.
 * - Batch fetches are deterministic: assets are de-duplicated case-insensitively
 *   in first-seen order, unsupported/invalid entries are skipped, and one bad
 *   entry can never remove or corrupt the other entries of the same batch.
 * - Cooldowns are driven only by an upstream HTTP 429, are bounded by
 *   MAX_COOLDOWN_MS so a hostile or buggy `Retry-After` cannot lock the provider
 *   out indefinitely, and never shrink an already-active cooldown.
 * - The API key is never logged, embedded in a URL, or included in an error
 *   payload; failures are reported through `describeFailure`, which emits only
 *   non-secret diagnostic fields.
 *
 * @see https://docs.coingecko.com/reference/simple-price
 */

import { BasePriceProvider, ProviderResponseError } from './base-provider.js';
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
 * Cooldown applied when CoinGecko answers 429 without a usable `Retry-After`.
 */
const DEFAULT_COOLDOWN_MS = 60_000;

/**
 * Upper bound for any cooldown derived from an upstream `Retry-After` header.
 * Without this bound a single malformed or hostile header (for example
 * `retry-after: 999999999999999999999`) would suspend the provider for
 * longer than the lifetime of the process, permanently disabling the
 * fallback price source.
 */
const MAX_COOLDOWN_MS = 300_000;

/**
 * Tolerance for `last_updated_at` landing slightly ahead of the local clock
 * (clock skew between the oracle host and CoinGecko).
 *
 * Only *future-dated* quotes are rejected here. An implausibly old quote is
 * left to `PriceValidator`, which owns the configurable staleness policy
 * (`priceStaleThresholdSeconds`); duplicating that policy inside the provider
 * would hard-code a threshold callers cannot tune. A future-dated quote is a
 * different class of problem: it would defeat the downstream timestamp
 * monotonicity and staleness checks entirely, pinning one stale quote as
 * "fresh" indefinitely, so it is rejected structurally here.
 */
const MAX_FUTURE_SKEW_SECONDS = 60;

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
 * Non-secret diagnostic view of a failure, safe to hand to the logger.
 *
 * Axios errors carry the full request config, which includes the
 * `x-cg-demo-api-key` / `x-cg-pro-api-key` header. Logging the raw error
 * object would therefore leak the CoinGecko API key into log output, so only
 * these explicitly whitelisted, non-sensitive fields are extracted.
 */
function describeFailure(error: unknown): Record<string, unknown> {
    if (error instanceof Error) {
        const details: Record<string, unknown> = { message: error.message };
        const code = (error as { code?: unknown }).code;
        if (typeof code === 'string') {
            details.code = code;
        }
        const status = (error as { response?: { status?: unknown } }).response?.status;
        if (typeof status === 'number') {
            details.status = status;
        }
        return details;
    }

    return { message: String(error) };
}

/**
 * Determine API tier from API key
 * - No key (or blank): Free tier
 * - Key starting with CG-: Demo tier
 * - Other key: Pro tier
 *
 * The key is trimmed first so a whitespace-only value degrades to the free
 * tier instead of being misrouted to the Pro host with a blank credential.
 */
function getApiTier(apiKey?: string): 'free' | 'demo' | 'pro' {
    const key = typeof apiKey === 'string' ? apiKey.trim() : '';
    if (!key) return 'free';
    if (key.startsWith('CG-')) return 'demo';
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
        this.apiKey = typeof config.apiKey === 'string' ? config.apiKey.trim() || undefined : undefined;
        this.tier = getApiTier(config.apiKey);

        logger.info('CoinGecko provider initialized', {
            tier: this.tier,
            baseUrl: config.baseUrl,
            // Only whether a key is configured, never the key itself.
            authenticated: Boolean(this.apiKey),
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
     *
     * @throws Error when the symbol has no CoinGecko mapping.
     */
    private getCoingeckoId(asset: string): string {
        const id = COINGECKO_ID_MAP[asset.toUpperCase()];
        if (!id) {
            throw new Error(`Asset ${asset} not mapped for CoinGecko`);
        }
        return id;
    }

    /**
     * Convert one CoinGecko quote into validated `RawPriceData`.
     *
     * Invariants:
     * - `usd` must be a finite number strictly greater than zero.
     * - `last_updated_at`, when supplied and non-zero, must be a positive
     *   integer epoch-seconds value that is not absurdly future-dated.
     * - An absent or zero `last_updated_at` falls back to the local clock, so
     *   the returned timestamp is always a positive integer.
     *
     * @throws ProviderResponseError when the payload cannot be trusted.
     */
    private parseCoinPrice(asset: string, coinId: string, coinData: unknown): RawPriceData {
        if (!coinData || typeof coinData !== 'object') {
            throw new ProviderResponseError(this.name, asset, `missing price data for ${coinId}`);
        }

        const quote = coinData as CoinGeckoSimplePriceResponse[string];
        const price = quote.usd;
        if (typeof price !== 'number' || !Number.isFinite(price) || price <= 0) {
            throw new ProviderResponseError(
                this.name,
                asset,
                `price must be a positive finite number, received ${String(price)}`,
            );
        }

        const raw = quote.last_updated_at;
        const nowSeconds = Math.floor(Date.now() / 1000);
        let timestamp = nowSeconds;

        if (raw !== undefined && raw !== null && raw !== 0) {
            if (typeof raw !== 'number' || !Number.isFinite(raw) || raw <= 0) {
                throw new ProviderResponseError(
                    this.name,
                    asset,
                    `last_updated_at must be a positive epoch-seconds number, received ${String(raw)}`,
                );
            }
            timestamp = Math.floor(raw);
            if (timestamp > nowSeconds + MAX_FUTURE_SKEW_SECONDS) {
                throw new ProviderResponseError(
                    this.name,
                    asset,
                    `last_updated_at ${timestamp} is ahead of the local clock`,
                );
            }
        }

        if (timestamp <= 0) {
            throw new ProviderResponseError(this.name, asset, 'resolved timestamp must be positive');
        }

        // Defence in depth: the base validator re-checks price/asset agreement
        // so a future edit to the checks above cannot silently emit bad data.
        return this.validateRawPrice(asset, {
            asset,
            price,
            timestamp,
            source: 'coingecko',
        });
    }

    /**
     * Build the outbound request headers. Returns a fresh object each call so
     * a caller can never mutate the stored credential.
     */
    private buildHeaders(): Record<string, string> {
        const headers: Record<string, string> = {};
        if (this.apiKey) {
            headers[this.getApiKeyHeader()] = this.apiKey;
        }
        return headers;
    }

    /**
     * Throw if the provider is currently suspended after a 429.
     */
    private assertNotCooledDown(): void {
        if (this.isCooledDown) {
            throw new Error(
                `CoinGecko provider is in cooldown until ${new Date(this.cooldownUntil).toISOString()}`,
            );
        }
    }

    /**
     * Fetch price for a specific asset
     *
     * @throws InvalidAssetError when `asset` is not a valid symbol.
     * @throws Error when the symbol has no CoinGecko mapping or a cooldown is active.
     * @throws ProviderResponseError when CoinGecko returns an unusable quote.
     */
    async fetchPrice(asset: string): Promise<RawPriceData> {
        const symbol = this.normalizeAsset(asset);
        const coinId = this.getCoingeckoId(symbol);

        this.assertNotCooledDown();

        await this.enforceRateLimit();

        const url = `${this.config.baseUrl}/simple/price?ids=${coinId}&vs_currencies=usd&include_last_updated_at=true`;

        try {
            const response = await this.request<CoinGeckoSimplePriceResponse>(url, {
                headers: this.buildHeaders(),
            });

            if (!response || typeof response !== 'object') {
                throw new ProviderResponseError(this.name, symbol, 'response body was not an object');
            }

            const coinData = response[coinId];
            if (!coinData) {
                throw new Error(`No price data returned for ${coinId}`);
            }

            return this.parseCoinPrice(symbol, coinId, coinData);
        } catch (error) {
            this.handleRateLimitError(error);
            logger.error(`CoinGecko fetch failed for ${symbol}`, describeFailure(error));
            throw error;
        }
    }

    /**
     * Fetch prices for multiple assets (batch API call)
     *
     * Partial failures are isolated: an unsupported input, a missing quote or
     * an unusable quote only removes that asset from the result. Duplicate
     * symbols (case-insensitive) are collapsed so the batch stays deterministic
     * and does not spend rate-limit budget twice on the same coin.
     */
    async fetchPrices(assets: string[]): Promise<RawPriceData[]> {
        if (!Array.isArray(assets)) {
            logger.warn('Invalid assets argument for CoinGecko, expected array');
            return [];
        }

        this.assertNotCooledDown();

        // Map all assets to CoinGecko IDs, preserving first-seen order.
        const assetToId: Map<string, string> = new Map();
        const validAssets: string[] = [];

        for (const asset of assets) {
            let symbol: string;
            try {
                symbol = this.normalizeAsset(asset);
            } catch {
                logger.warn(`Skipping invalid asset for CoinGecko`, { asset });
                continue;
            }

            try {
                const id = this.getCoingeckoId(symbol);
                if (assetToId.has(symbol)) {
                    logger.warn(`Skipping duplicate asset for CoinGecko`, { asset: symbol });
                    continue;
                }
                assetToId.set(symbol, id);
                validAssets.push(symbol);
            } catch {
                logger.warn(`Skipping unsupported asset: ${symbol}`);
            }
        }

        if (validAssets.length === 0) {
            return [];
        }

        await this.enforceRateLimit();

        const coinIds = validAssets.map((a) => assetToId.get(a)!).join(',');
        const url = `${this.config.baseUrl}/simple/price?ids=${coinIds}&vs_currencies=usd&include_last_updated_at=true`;

        try {
            const response = await this.request<CoinGeckoSimplePriceResponse>(url, {
                headers: this.buildHeaders(),
            });

            if (!response || typeof response !== 'object') {
                throw new ProviderResponseError(
                    this.name,
                    validAssets.join(','),
                    'response body was not an object',
                );
            }

            const results: RawPriceData[] = [];

            for (const asset of validAssets) {
                const coinId = assetToId.get(asset)!;
                const coinData = response[coinId];

                if (!coinData) {
                    logger.warn(`CoinGecko omitted ${coinId} from the batch response`, { asset });
                    continue;
                }

                try {
                    results.push(this.parseCoinPrice(asset, coinId, coinData));
                } catch (error) {
                    logger.warn(`Skipping unusable CoinGecko quote for ${asset}`, describeFailure(error));
                }
            }

            return results;
        } catch (error) {
            this.handleRateLimitError(error);
            logger.error('CoinGecko batch fetch failed', describeFailure(error));
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
     *
     * Per RFC 9110 an HTTP-date always begins with a letter (the day name), so
     * only values matching that shape are handed to `Date.parse`. This matters:
     * JavaScript's date parser is far more lenient than the RFC and happily
     * turns non-date input such as `1.5` or `+30` into a date in the year 2001,
     * which previously collapsed the suspension to zero and let the provider
     * immediately hammer an upstream that had just asked it to back off.
     *
     * Returns a delay in milliseconds clamped to [0, MAX_COOLDOWN_MS], or null
     * when the header is absent or malformed so the caller falls back to
     * DEFAULT_COOLDOWN_MS.
     */
    private parseRetryAfter(headerValue?: string | string[]): number | null {
        if (!headerValue) return null;
        const valueStr = Array.isArray(headerValue) ? headerValue[0] : headerValue;
        if (!valueStr || typeof valueStr !== 'string') return null;

        const trimmed = valueStr.trim();
        if (trimmed.length === 0) return null;

        // delay-seconds = 1*DIGIT
        if (/^\d+$/.test(trimmed)) {
            // The regex guarantees a parseable (possibly huge) integer, so only
            // the clamp below is needed; a syntactically valid but absurd delay
            // is clamped, not rejected, so the provider is suspended for
            // exactly MAX_COOLDOWN_MS.
            return Math.min(Number(trimmed) * 1000, MAX_COOLDOWN_MS);
        }

        // Anything else must be an HTTP-date to be honoured at all.
        if (!/^[A-Za-z]/.test(trimmed)) {
            return null;
        }

        const parsedDate = Date.parse(trimmed);
        if (Number.isNaN(parsedDate)) {
            return null;
        }

        const ms = parsedDate - Date.now();
        return Math.min(Math.max(ms, 0), MAX_COOLDOWN_MS);
    }

    /**
     * Inspects error and sets cooldown if 429 rate limited.
     *
     * The cooldown is only ever extended, never shortened, so a late or
     * malformed `Retry-After` on a subsequent 429 cannot cut an active
     * suspension short and resume hammering the upstream.
     */
    private handleRateLimitError(error: unknown): void {
        const status = (error as { response?: { status?: unknown } } | null)?.response?.status;
        if (status !== 429) {
            return;
        }

        const retryAfterHeader = (error as { response?: { headers?: Record<string, unknown> } })
            ?.response?.headers?.['retry-after'] as string | string[] | undefined;
        const parsed = this.parseRetryAfter(retryAfterHeader);
        const delayMs = parsed ?? DEFAULT_COOLDOWN_MS;
        const candidate = Date.now() + delayMs;

        if (candidate > this.cooldownUntil) {
            this.cooldownUntil = candidate;
        }

        logger.warn(
            `CoinGecko rate limited (429). Suspending provider for ${delayMs}ms (until ${new Date(this.cooldownUntil).toISOString()})`,
            { retryAfter: Array.isArray(retryAfterHeader) ? retryAfterHeader[0] : retryAfterHeader },
        );
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
        apiKey: typeof apiKey === 'string' ? apiKey.trim() || undefined : undefined,
        baseUrl,
        rateLimit: {
            maxRequests: tier === 'free' ? 10 : 500,
            windowMs: 60000,
        },
    };

    return new CoinGeckoProvider(config);
}
