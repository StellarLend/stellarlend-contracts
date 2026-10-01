/**
 * Failure-path and boundary coverage for `src/providers/coingecko.ts`.
 *
 * The invariants exercised here are the ones documented at the top of the
 * source module:
 *
 *   I-1  Input validation  - asset symbols are normalized/validated before any
 *                            I/O; invalid input never reaches CoinGecko and
 *                            never consumes rate-limit budget.
 *   I-2  Response validation - a quote is only promoted to RawPriceData when it
 *                            carries a finite, strictly positive USD price and
 *                            a plausible timestamp. Malformed payloads must
 *                            never become a fabricated or zero price.
 *   I-3  Deterministic batch - case-insensitive de-duplication in first-seen
 *                            order; one bad entry cannot remove or corrupt the
 *                            other entries of the same batch.
 *   I-4  Cooldown safety - only an upstream 429 suspends the provider, the
 *                            suspension is bounded, and an already-active
 *                            cooldown is never shortened.
 *   I-5  Request amplification - exactly one upstream request per attempt.
 *   I-6  Secret hygiene - the API key never reaches logs, URLs or errors.
 *
 * Determinism strategy:
 * - The wall clock is pinned with `vi.setSystemTime`, so timestamp boundaries
 *   are exact and no assertion depends on real elapsed time.
 * - The logger is mocked so log assertions cannot interleave with test output
 *   and secret-leak assertions are deterministic.
 * - `axios.isAxiosError` is driven explicitly per test, which is what makes
 *   retry / cooldown branches reachable and distinguishable.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { CoinGeckoProvider, createCoinGeckoProvider } from '../src/providers/coingecko.js';
import { InvalidAssetError, ProviderResponseError } from '../src/providers/base-provider.js';
import { validateRawPriceData } from '../src/types/index.js';
import { logger } from '../src/utils/logger.js';
import type { ProviderConfig } from '../src/types/index.js';
import axios from 'axios';

vi.mock('axios', () => ({
    default: {
        get: vi.fn(),
        isAxiosError: vi.fn(() => false),
    },
}));

vi.mock('../src/utils/logger.js', () => ({
    logger: {
        debug: vi.fn(),
        info: vi.fn(),
        warn: vi.fn(),
        error: vi.fn(),
    },
}));

const mockedAxios = vi.mocked(axios);
const mockedLogger = vi.mocked(logger);

/** Fixed reference instant for every time-based assertion. */
const T0 = 1_760_000_000_000;

/** Epoch seconds matching T0. */
const T0_SECONDS = T0 / 1000;

/** Mock a CoinGecko-shaped axios rejection carrying an HTTP status. */
function httpError(status: number, headers: Record<string, string | string[]> = {}): Error {
    const error = new Error(`Request failed with status code ${status}`) as Error & {
        code?: string;
        response: { status: number; headers: Record<string, string | string[]> };
    };
    error.code = `ERR_${status}`;
    error.response = { status, headers };
    return error;
}

/** An axios-style error that the retry layer treats as transient (no response). */
function networkError(message = 'ECONNRESET'): Error {
    const error = new Error(message) as Error & { code?: string };
    error.code = message;
    return error;
}

/** Resolve the next upstream call with a CoinGecko simple-price body. */
function replyWith(body: unknown): void {
    mockedAxios.get.mockResolvedValueOnce({ data: body } as never);
}

/** Build a provider with an explicit rate-limit budget. */
function buildProvider(
    rateLimit = { maxRequests: 10, windowMs: 60_000 },
    apiKey?: string,
): CoinGeckoProvider {
    const config: ProviderConfig = {
        name: 'coingecko',
        enabled: true,
        priority: 1,
        weight: 0.6,
        baseUrl: 'https://api.coingecko.com/api/v3',
        rateLimit,
    };
    if (apiKey !== undefined) {
        config.apiKey = apiKey;
    }
    return new CoinGeckoProvider(config);
}

/** Every argument the provider handed to axios, joined for substring checks. */
function allAxiosArgs(): string {
    return JSON.stringify(mockedAxios.get.mock.calls);
}

/** Every log call the provider made, serialized for substring checks. */
function allLogOutput(): string {
    const entries = [
        mockedLogger.debug.mock.calls,
        mockedLogger.info.mock.calls,
        mockedLogger.warn.mock.calls,
        mockedLogger.error.mock.calls,
    ];
    return JSON.stringify(entries);
}

/**
 * Drain fake timers so bounded retry/back-off loops can run to completion.
 * Each `request` attempt sleeps on the fake clock, so a single tick is not
 * enough: the loop re-enters `axios.get` only after the awaited sleep settles.
 */
async function settle(): Promise<void> {
    for (let i = 0; i < 25; i += 1) {
        await vi.advanceTimersByTimeAsync(1_000);
    }
}

beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(T0);
    vi.clearAllMocks();
    mockedAxios.isAxiosError.mockReturnValue(false);
});

afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
});

// ---------------------------------------------------------------------------
// I-1 / I-2: input validation happens before I/O
// ---------------------------------------------------------------------------
describe('I-1 input validation rejects before any network I/O', () => {
    const invalidAssets: Array<[string, unknown]> = [
        ['empty string', ''],
        ['whitespace only', '   '],
        ['null', null],
        ['undefined', undefined],
        ['number', 42],
        ['object', { symbol: 'XLM' }],
        ['array', ['XLM']],
        ['illegal separator', 'XLM/USD'],
        ['injection attempt', 'XLM?ids=evil'],
        ['embedded whitespace', 'X LM'],
        ['33 characters (over the 32 limit)', 'A'.repeat(33)],
    ];

    it.each(invalidAssets)('rejects %s', async (_label, asset) => {
        const provider = buildProvider();

        await expect(provider.fetchPrice(asset as string)).rejects.toBeInstanceOf(InvalidAssetError);
        expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('accepts a 32-character identifier for the charset check but still fails mapping', async () => {
        const provider = buildProvider();
        const atLength = 'A'.repeat(32);

        // 32 characters is inside the length boundary; it simply has no mapping.
        await expect(provider.fetchPrice(atLength)).rejects.toThrow('not mapped for CoinGecko');
        expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('normalizes surrounding whitespace and case before mapping', async () => {
        const provider = buildProvider();
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        const result = await provider.fetchPrice('  xlm  ');

        expect(result.asset).toBe('XLM');
        expect(mockedAxios.get).toHaveBeenCalledTimes(1);
        expect(mockedAxios.get.mock.calls[0][0]).toContain('ids=stellar');
    });

    it('does not consume rate-limit budget for a rejected symbol', async () => {
        const provider = buildProvider({ maxRequests: 1, windowMs: 60_000 });

        await expect(provider.fetchPrice('UNKNOWN')).rejects.toThrow(
            'Asset UNKNOWN not mapped for CoinGecko',
        );

        // The single available slot is untouched, so the next call still runs.
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });
        await expect(provider.fetchPrice('XLM')).resolves.toMatchObject({ price: 0.15 });
        expect(mockedAxios.get).toHaveBeenCalledTimes(1);
    });
});

// ---------------------------------------------------------------------------
// I-2: response validation
// ---------------------------------------------------------------------------
describe('I-2 unusable quotes are never promoted to RawPriceData', () => {
    const rejectedQuotes: Array<[string, unknown]> = [
        ['zero price', { usd: 0 }],
        ['negative price', { usd: -1 }],
        ['NaN price', { usd: Number.NaN }],
        ['Infinity price', { usd: Number.POSITIVE_INFINITY }],
        ['numeric string price', { usd: '0.15' }],
        ['null price', { usd: null }],
        ['missing price field', {}],
        ['array quote object', []],
    ];

    it.each(rejectedQuotes)('fetchPrice rejects a quote with %s', async (_label, quote) => {
        const provider = buildProvider();
        replyWith({ stellar: quote });

        await expect(provider.fetchPrice('XLM')).rejects.toBeInstanceOf(ProviderResponseError);
    });

    it('treats a null quote as an absent entry', async () => {
        const provider = buildProvider();
        replyWith({ stellar: null });

        await expect(provider.fetchPrice('XLM')).rejects.toThrow(
            'No price data returned for stellar',
        );
    });

    it.each([
        ['string', '0.15'],
        ['number', 0.15],
        ['boolean', true],
    ])('rejects a %s used in place of a quote object', async (_label, quote) => {
        const provider = buildProvider();
        replyWith({ stellar: quote });

        await expect(provider.fetchPrice('XLM')).rejects.toBeInstanceOf(ProviderResponseError);
    });

    it.each([
        ['string', '0.15'],
        ['number', 0.15],
        ['boolean', true],
    ])('drops a %s used in place of a quote object from a batch', async (_label, quote) => {
        const provider = buildProvider();
        replyWith({
            stellar: quote,
            bitcoin: { usd: 50_000, last_updated_at: T0_SECONDS },
        });

        const results = await provider.fetchPrices(['XLM', 'BTC']);

        expect(results.map((r) => r.asset)).toEqual(['BTC']);
    });

    it.each(rejectedQuotes)('batch fetchPrices drops a quote with %s but keeps the rest', async (_label, quote) => {
        const provider = buildProvider();
        replyWith({ stellar: quote, bitcoin: { usd: 50_000, last_updated_at: T0_SECONDS } });

        const results = await provider.fetchPrices(['XLM', 'BTC']);

        expect(results.map((r) => r.asset)).toEqual(['BTC']);
        expect(validateRawPriceData(results[0]).isValid).toBe(true);
    });

    const rejectedTimestamps: Array<[string, unknown]> = [
        ['negative', -1],
        ['NaN', Number.NaN],
        ['Infinity', Number.POSITIVE_INFINITY],
        ['numeric string', '1760000000'],
        ['sub-second, which truncates to epoch zero', 0.5],
        ['more than the skew tolerance in the future', T0_SECONDS + 3_600],
    ];

    it.each(rejectedTimestamps)('rejects a last_updated_at that is %s', async (_label, timestamp) => {
        const provider = buildProvider();
        replyWith({ stellar: { usd: 0.15, last_updated_at: timestamp } });

        await expect(provider.fetchPrice('XLM')).rejects.toBeInstanceOf(ProviderResponseError);
    });

    it('rejects a quote when the local clock sits at the epoch boundary', async () => {
        vi.setSystemTime(0);
        const provider = buildProvider();
        // With no usable last_updated_at the provider falls back to the local
        // clock, which is not a positive epoch-seconds value at time zero.
        replyWith({ stellar: { usd: 0.15 } });

        await expect(provider.fetchPrice('XLM')).rejects.toBeInstanceOf(ProviderResponseError);
    });

    it('accepts a last_updated_at at the exact future-skew boundary', async () => {
        const provider = buildProvider();
        const atBoundary = T0_SECONDS + 60;
        replyWith({ stellar: { usd: 0.15, last_updated_at: atBoundary } });

        await expect(provider.fetchPrice('XLM')).resolves.toMatchObject({
            timestamp: atBoundary,
        });
    });

    it.each([
        ['absent', undefined],
        ['null', null],
        ['zero', 0],
    ])('falls back to the local clock when last_updated_at is %s', async (_label, lastUpdatedAt) => {
        const provider = buildProvider();
        replyWith({ stellar: { usd: 0.15, last_updated_at: lastUpdatedAt } });

        await expect(provider.fetchPrice('XLM')).resolves.toMatchObject({
            timestamp: T0_SECONDS,
        });
    });

    it('truncates a fractional last_updated_at to whole epoch seconds', async () => {
        const provider = buildProvider();
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS + 0.75 } });

        const result = await provider.fetchPrice('XLM');

        expect(result.timestamp).toBe(T0_SECONDS);
        expect(Number.isInteger(result.timestamp)).toBe(true);
    });

    it('rejects a non-object response body', async () => {
        const provider = buildProvider();
        replyWith(null);

        await expect(provider.fetchPrice('XLM')).rejects.toBeInstanceOf(ProviderResponseError);
    });

    it('reports a missing coin entry with the documented message', async () => {
        const provider = buildProvider();
        replyWith({ bitcoin: { usd: 50_000 } });

        await expect(provider.fetchPrice('XLM')).rejects.toThrow('No price data returned for stellar');
    });

    it('always produces RawPriceData that passes validateRawPriceData', async () => {
        const provider = buildProvider();
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        const result = await provider.fetchPrice('XLM');

        expect(validateRawPriceData(result).isValid).toBe(true);
    });
});

// ---------------------------------------------------------------------------
// I-3: deterministic batch behaviour
// ---------------------------------------------------------------------------
describe('I-3 batch fetchPrices is deterministic and failure-isolated', () => {
    it('de-duplicates case-insensitively while preserving first-seen order', async () => {
        const provider = buildProvider();
        replyWith({
            bitcoin: { usd: 50_000, last_updated_at: T0_SECONDS },
            stellar: { usd: 0.15, last_updated_at: T0_SECONDS },
        });

        const results = await provider.fetchPrices(['XLM', 'xlm', ' BTC ', 'btc', 'XLM']);

        expect(results.map((r) => r.asset)).toEqual(['XLM', 'BTC']);
        expect(mockedAxios.get).toHaveBeenCalledTimes(1);
        // One request per distinct coin: the id list is not padded with repeats.
        expect(mockedAxios.get.mock.calls[0][0]).toContain('ids=stellar,bitcoin');
    });

    it('returns an empty array and issues no request when nothing is mappable', async () => {
        const provider = buildProvider();

        await expect(provider.fetchPrices(['NOPE', 'ALSOWRONG'])).resolves.toEqual([]);
        expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('returns an empty array for an empty batch without spending budget', async () => {
        const provider = buildProvider({ maxRequests: 1, windowMs: 60_000 });

        await expect(provider.fetchPrices([])).resolves.toEqual([]);
        expect(mockedAxios.get).not.toHaveBeenCalled();

        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });
        await expect(provider.fetchPrice('XLM')).resolves.toMatchObject({ price: 0.15 });
    });

    it('skips non-array input instead of throwing', async () => {
        const provider = buildProvider();

        await expect(
            provider.fetchPrices('XLM' as unknown as string[]),
        ).resolves.toEqual([]);
        await expect(
            provider.fetchPrices(null as unknown as string[]),
        ).resolves.toEqual([]);
        expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('skips invalid and unmappable entries without failing the batch', async () => {
        const provider = buildProvider();
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        const results = await provider.fetchPrices([
            null,
            123,
            '',
            'UNKNOWN',
            'XLM',
        ]);

        expect(results.map((r) => r.asset)).toEqual(['XLM']);
        expect(mockedAxios.get.mock.calls[0][0]).toContain('ids=stellar');
    });

    it('omits an asset that CoinGecko left out of the batch body', async () => {
        const provider = buildProvider();
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        const results = await provider.fetchPrices(['XLM', 'BTC']);

        expect(results.map((r) => r.asset)).toEqual(['XLM']);
    });

    it('rejects the whole batch when the upstream body is not an object', async () => {
        const provider = buildProvider();
        replyWith('rate limited');

        await expect(provider.fetchPrices(['XLM', 'BTC'])).rejects.toBeInstanceOf(
            ProviderResponseError,
        );
    });

    it('isolates a non-Error rejection instead of masking it', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce('socket hang up' as never);

        await expect(provider.fetchPrices(['XLM'])).rejects.toBe('socket hang up');
        expect(mockedLogger.error).toHaveBeenCalled();
    });

    it('is repeatable: identical input yields an identical result', async () => {
        const provider = buildProvider();
        const body = {
            stellar: { usd: 0.15, last_updated_at: T0_SECONDS },
            bitcoin: { usd: 50_000, last_updated_at: T0_SECONDS },
        };
        replyWith(body);
        const first = await provider.fetchPrices(['BTC', 'XLM']);
        replyWith(body);
        const second = await provider.fetchPrices(['BTC', 'XLM']);

        expect(second).toEqual(first);
    });
});

// ---------------------------------------------------------------------------
// I-4: cooldown safety
// ---------------------------------------------------------------------------
describe('I-4 cooldown transitions are bounded and monotonic', () => {
    // `axios.isAxiosError` is false in this block so the base retry layer does
    // not also derive a cooldown from the same 429. That isolates CoinGecko's
    // own `Retry-After` policy so each boundary can be asserted exactly, and a
    // non-axios rejection is surfaced on the first attempt without needing the
    // fake clock to be advanced.
    beforeEach(() => {
        mockedAxios.isAxiosError.mockReturnValue(false);
    });

    it('does not suspend on non-429 failures', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(httpError(500));

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(provider.isCooledDown).toBe(false);
        expect(provider.cooldownUntil).toBe(0);
    });

    it('does not suspend when the failure carries no response at all', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(networkError());

        await expect(provider.fetchPrice('XLM')).rejects.toThrow('ECONNRESET');

        expect(provider.isCooledDown).toBe(false);
    });

    it('applies the default cooldown when Retry-After is absent', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(httpError(429));

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(provider.isCooledDown).toBe(true);
        expect(provider.cooldownUntil).toBe(T0 + 60_000);
    });

    const malformedRetryAfter: Array<[string, string]> = [
        ['fractional seconds', '1.5'],
        ['negative seconds', '-5'],
        ['signed seconds', '+30'],
        ['non-numeric', 'soon'],
        ['empty string', ''],
        ['whitespace only', '   '],
        ['trailing garbage', '30abc'],
        ['exponent notation', '1e3'],
    ];

    it.each(malformedRetryAfter)(
        'falls back to the default cooldown for a malformed Retry-After (%s)',
        async (_label, headerValue) => {
            const provider = buildProvider();
            mockedAxios.get.mockRejectedValueOnce(httpError(429, { 'retry-after': headerValue }));

            await expect(provider.fetchPrice('XLM')).rejects.toThrow();

            // A malformed value must never degrade the suspension towards zero.
            // `Date.parse('1.5')` is a valid date in V8, which previously
            // produced a 0ms cooldown and let the provider immediately hammer an
            // upstream that had just asked it to back off.
            expect(provider.cooldownUntil).toBe(T0 + 60_000);
        },
    );

    it('clamps an absurd Retry-After so a hostile header cannot lock the provider out', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(
            httpError(429, { 'retry-after': '999999999999999999999' }),
        );

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(provider.cooldownUntil).toBe(T0 + 300_000);
    });

    it('clamps a Retry-After of a full day to the maximum', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(httpError(429, { 'retry-after': '86400' }));

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(provider.cooldownUntil).toBe(T0 + 300_000);
    });

    it('accepts zero as the lower boundary', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(httpError(429, { 'retry-after': '0' }));

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(provider.cooldownUntil).toBe(T0);
        expect(provider.isCooledDown).toBe(false);
    });

    it('uses the first value when Retry-After arrives as an array', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(httpError(429, { 'retry-after': ['45', '90'] }));

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(provider.cooldownUntil).toBe(T0 + 45_000);
    });

    it('uses the default cooldown when the Retry-After array is empty', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(httpError(429, { 'retry-after': [] }));

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(provider.cooldownUntil).toBe(T0 + 60_000);
    });

    it('resolves an HTTP-date Retry-After to the delay it encodes', async () => {
        const provider = buildProvider();
        const retryAt = new Date(T0 + 45_000);
        mockedAxios.get.mockRejectedValueOnce(
            httpError(429, { 'retry-after': retryAt.toUTCString() }),
        );

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(provider.cooldownUntil).toBe(retryAt.getTime());
    });

    it('clamps an HTTP-date Retry-After to the maximum', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(
            httpError(429, { 'retry-after': new Date(T0 + 86_400_000).toUTCString() }),
        );

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(provider.cooldownUntil).toBe(T0 + 300_000);
    });

    it('resumes immediately when the HTTP-date is already in the past', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(
            httpError(429, { 'retry-after': new Date(T0 - 60_000).toUTCString() }),
        );

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(provider.cooldownUntil).toBe(T0);
        expect(provider.isCooledDown).toBe(false);
    });

    it('never shortens an active cooldown on a later 429', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(httpError(429, { 'retry-after': '300' }));
        await expect(provider.fetchPrice('XLM')).rejects.toThrow();
        const firstCooldown = provider.cooldownUntil;
        expect(firstCooldown).toBe(T0 + 300_000);

        // A second 429 asks for only 1s; the longer suspension must win so a
        // late header cannot resume hammering the upstream early.
        vi.setSystemTime(T0 + 1_000);
        mockedAxios.get.mockRejectedValueOnce(httpError(429, { 'retry-after': '1' }));
        await expect(provider.fetchPrice('BTC')).rejects.toThrow();

        expect(provider.cooldownUntil).toBe(firstCooldown);
    });

    it('blocks both entry points while cooled down and issues no request', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(httpError(429, { 'retry-after': '30' }));
        await expect(provider.fetchPrice('XLM')).rejects.toThrow();
        const callsAfterCooldown = mockedAxios.get.mock.calls.length;

        await expect(provider.fetchPrice('BTC')).rejects.toThrow(/cooldown/);
        await expect(provider.fetchPrices(['XLM', 'BTC'])).rejects.toThrow(/cooldown/);
        expect(mockedAxios.get).toHaveBeenCalledTimes(callsAfterCooldown);
    });

    it('resumes fetching once the cooldown boundary is exactly reached', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(httpError(429, { 'retry-after': '30' }));
        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        vi.setSystemTime(T0 + 29_999);
        expect(provider.isCooledDown).toBe(true);
        await expect(provider.fetchPrice('XLM')).rejects.toThrow(/cooldown/);

        vi.setSystemTime(T0 + 30_000);
        expect(provider.isCooledDown).toBe(false);
        replyWith({ stellar: { usd: 0.16, last_updated_at: T0_SECONDS } });
        await expect(provider.fetchPrice('XLM')).resolves.toMatchObject({ price: 0.16 });
    });

    it('rejects invalid input with InvalidAssetError even while cooled down', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(httpError(429));
        await expect(provider.fetchPrice('XLM')).rejects.toThrow();
        mockedAxios.get.mockClear();

        // Validation precedes cooldown so the rejection reason is deterministic.
        await expect(provider.fetchPrice('')).rejects.toBeInstanceOf(InvalidAssetError);
        expect(mockedAxios.get).not.toHaveBeenCalled();
    });
});

describe('I-4 cooldown stays safe when the base retry layer also acts', () => {
    // `BasePriceProvider.request` derives its own cooldown for genuine axios 429
    // errors. Both layers must agree on the safety envelope: a well-formed
    // delay is honoured exactly, a malformed one never collapses to an
    // immediate retry, and nothing is ever unbounded.
    it.each([
        ['below the default', '30', T0 + 30_000],
        ['at the maximum', '300', T0 + 300_000],
        ['above the maximum', '86400', T0 + 300_000],
    ])('honours a well-formed %s Retry-After', async (_label, retryAfter, expected) => {
        mockedAxios.isAxiosError.mockReturnValue(true);
        const provider = buildProvider();
        mockedAxios.get.mockRejectedValueOnce(httpError(429, { 'retry-after': retryAfter }));

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(provider.cooldownUntil).toBe(expected);
    });

    it.each([
        ['malformed', '1.5'],
        ['unparseable', 'soon'],
        ['absent', undefined],
    ])('never collapses a %s Retry-After to an immediate retry', async (_label, retryAfter) => {
        mockedAxios.isAxiosError.mockReturnValue(true);
        const provider = buildProvider();
        const headers = retryAfter === undefined ? {} : { 'retry-after': retryAfter };
        mockedAxios.get.mockRejectedValueOnce(httpError(429, headers));

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(provider.cooldownUntil).toBeGreaterThanOrEqual(T0 + 60_000);
        expect(provider.cooldownUntil).toBeLessThanOrEqual(T0 + 300_000);
    });
});
describe('I-5 retries stay bounded and never amplify load', () => {
    it('issues exactly one upstream request for a successful fetchPrice', async () => {
        const provider = buildProvider();
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        await provider.fetchPrice('XLM');

        expect(mockedAxios.get).toHaveBeenCalledTimes(1);
    });

    it('issues exactly one upstream request for a successful batch', async () => {
        const provider = buildProvider();
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        await provider.fetchPrices(['XLM', 'BTC']);

        expect(mockedAxios.get).toHaveBeenCalledTimes(1);
    });

    it('retries a transient network failure and recovers on the second attempt', async () => {
        const provider = buildProvider();
        mockedAxios.isAxiosError.mockReturnValue(true);
        mockedAxios.get.mockRejectedValueOnce(networkError());
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        const pending = provider.fetchPrice('XLM');
        await vi.advanceTimersByTimeAsync(250);
        const result = await pending;

        expect(result.price).toBe(0.15);
        expect(mockedAxios.get).toHaveBeenCalledTimes(2);
    });

    it('gives up after the third attempt and surfaces the failure', async () => {
        const provider = buildProvider();
        mockedAxios.isAxiosError.mockReturnValue(true);
        mockedAxios.get.mockRejectedValue(httpError(500));

        const pending = provider.fetchPrice('XLM');
        const assertion = expect(pending).rejects.toThrow('status code 500');
        await settle();
        await assertion;

        // Bounded: exactly MAX_REQUEST_ATTEMPTS upstream calls, never more.
        expect(mockedAxios.get).toHaveBeenCalledTimes(3);
    });

    it('does not retry a 503, which the retry layer treats as a rate limit', async () => {
        const provider = buildProvider();
        mockedAxios.isAxiosError.mockReturnValue(true);
        mockedAxios.get.mockRejectedValueOnce(httpError(503));

        await expect(provider.fetchPrice('XLM')).rejects.toThrow('status code 503');

        expect(mockedAxios.get).toHaveBeenCalledTimes(1);
        expect(provider.isCooledDown).toBe(true);
    });

    it('keeps the retry budget from being amplified across repeated failures', async () => {
        const provider = buildProvider();
        mockedAxios.isAxiosError.mockReturnValue(true);
        mockedAxios.get.mockRejectedValue(httpError(500));

        const first = provider.fetchPrice('XLM');
        const firstAssertion = expect(first).rejects.toThrow('status code 500');
        await settle();
        await firstAssertion;
        expect(mockedAxios.get).toHaveBeenCalledTimes(3);

        mockedAxios.get.mockClear();
        const second = provider.fetchPrice('XLM');
        const secondAssertion = expect(second).rejects.toThrow('status code 500');
        await settle();
        await secondAssertion;
        expect(mockedAxios.get).toHaveBeenCalledTimes(3);
    });

    it('does not retry a 4xx client error', async () => {
        const provider = buildProvider();
        mockedAxios.isAxiosError.mockReturnValue(true);
        mockedAxios.get.mockRejectedValueOnce(httpError(404));

        await expect(provider.fetchPrice('XLM')).rejects.toThrow('status code 404');

        expect(mockedAxios.get).toHaveBeenCalledTimes(1);
        expect(provider.isCooledDown).toBe(false);
    });

    it('does not retry a 429 but does suspend the provider', async () => {
        const provider = buildProvider();
        mockedAxios.isAxiosError.mockReturnValue(true);
        mockedAxios.get.mockRejectedValueOnce(httpError(429, { 'retry-after': '30' }));

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(mockedAxios.get).toHaveBeenCalledTimes(1);
        expect(provider.isCooledDown).toBe(true);
    });

    it('recovers on the next call after a 429 cooldown elapses', async () => {
        const provider = buildProvider();
        mockedAxios.isAxiosError.mockReturnValue(true);
        mockedAxios.get.mockRejectedValueOnce(httpError(429, { 'retry-after': '10' }));
        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        vi.setSystemTime(T0 + 10_000);
        replyWith({ stellar: { usd: 0.16, last_updated_at: T0_SECONDS } });

        await expect(provider.fetchPrice('XLM')).resolves.toMatchObject({ price: 0.16 });
        expect(mockedAxios.get).toHaveBeenCalledTimes(2);
    });
});

// ---------------------------------------------------------------------------
// Concurrency: concurrent callers must not bypass rate limiting or corrupt state
// ---------------------------------------------------------------------------
describe('concurrency stays within the rate-limit budget and keeps results isolated', () => {
    it('serves concurrent fetches for different assets independently', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockImplementation((url: string) =>
            Promise.resolve({
                data: {
                    stellar: { usd: 0.15, last_updated_at: T0_SECONDS },
                    bitcoin: { usd: 50_000, last_updated_at: T0_SECONDS },
                },
            } as never),
        );

        const [xlm, btc] = await Promise.all([
            provider.fetchPrice('XLM'),
            provider.fetchPrice('BTC'),
        ]);

        expect(xlm).toMatchObject({ asset: 'XLM', price: 0.15 });
        expect(btc).toMatchObject({ asset: 'BTC', price: 50_000 });
    });

    it('does not let one failing concurrent call corrupt the successful one', async () => {
        const provider = buildProvider();
        // Non-retryable framing keeps the assertion about result isolation
        // rather than about the retry/back-off clock.
        mockedAxios.isAxiosError.mockReturnValue(false);
        mockedAxios.get.mockImplementation((url: string) => {
            if (String(url).includes('bitcoin')) {
                return Promise.reject(new Error('socket hang up'));
            }
            return Promise.resolve({
                data: { stellar: { usd: 0.15, last_updated_at: T0_SECONDS } },
            } as never);
        });

        const settled = await Promise.allSettled([
            provider.fetchPrice('XLM'),
            provider.fetchPrice('BTC'),
        ]);

        expect(settled[0].status).toBe('fulfilled');
        expect(settled[1].status).toBe('rejected');
        expect((settled[0] as PromiseFulfilledResult<{ price: number }>).value.price).toBe(0.15);
    });

    it('keeps concurrent batch and single fetches from interfering', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockResolvedValue({
            data: {
                stellar: { usd: 0.15, last_updated_at: T0_SECONDS },
                bitcoin: { usd: 50_000, last_updated_at: T0_SECONDS },
            },
        } as never);

        const [batch, single] = await Promise.all([
            provider.fetchPrices(['XLM', 'BTC']),
            provider.fetchPrice('BTC'),
        ]);

        expect(batch.map((r) => r.asset)).toEqual(['XLM', 'BTC']);
        expect(single).toMatchObject({ asset: 'BTC', price: 50_000 });
    });

    it('issues one upstream request per concurrent successful caller', async () => {
        const provider = buildProvider();
        mockedAxios.get.mockResolvedValue({
            data: { stellar: { usd: 0.15, last_updated_at: T0_SECONDS } },
        } as never);

        await Promise.all([
            provider.fetchPrice('XLM'),
            provider.fetchPrice('XLM'),
            provider.fetchPrice('XLM'),
        ]);

        expect(mockedAxios.get).toHaveBeenCalledTimes(3);
    });

    it('serializes admission so concurrent calls cannot exceed the budget', async () => {
        const provider = buildProvider({ maxRequests: 2, windowMs: 60_000 });
        mockedAxios.get.mockResolvedValue({
            data: { stellar: { usd: 0.15, last_updated_at: T0_SECONDS } },
        } as never);

        const first = Promise.all([provider.fetchPrice('XLM'), provider.fetchPrice('XLM')]);
        await vi.advanceTimersByTimeAsync(0);
        await first;

        // Two slots consumed; the third caller must wait out the window.
        const third = provider.fetchPrice('XLM');
        let settledEarly = false;
        void third.then(() => {
            settledEarly = true;
        });
        await vi.advanceTimersByTimeAsync(59_999);
        expect(settledEarly).toBe(false);

        await vi.advanceTimersByTimeAsync(1);
        await third;
        expect(settledEarly).toBe(true);
    });
});

// ---------------------------------------------------------------------------
// I-6: secret hygiene
// ---------------------------------------------------------------------------
describe('I-6 the API key never leaks into logs, URLs or errors', () => {
    const SECRET = 'CG-super-secret-demo-key';

    function providerWithKey(key?: string): CoinGeckoProvider {
        return createCoinGeckoProvider(key);
    }

    function firstRequestArgs(): { url: string; headers: Record<string, string> } {
        const call = mockedAxios.get.mock.calls[0] as unknown as [
            string,
            { headers: Record<string, string> },
        ];
        return { url: call[0], headers: call[1].headers };
    }

    it('sends the demo key in the demo header on the shared host', async () => {
        const provider = providerWithKey(SECRET);
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        await provider.fetchPrice('XLM');

        const { url, headers } = firstRequestArgs();
        expect(url).toContain('api.coingecko.com');
        expect(url).not.toContain('pro-api');
        expect(headers['x-cg-demo-api-key']).toBe(SECRET);
        expect(headers['x-cg-pro-api-key']).toBeUndefined();
    });

    it('never places the key in the query string', async () => {
        const provider = providerWithKey(SECRET);
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        await provider.fetchPrice('XLM');

        const { url } = firstRequestArgs();
        expect(url).not.toContain(SECRET);
        expect(url).not.toContain('api-key');
        expect(url).not.toContain('api_key');
    });

    it('does not leak the key when the request fails', async () => {
        const provider = providerWithKey(SECRET);
        const failure = httpError(429, { 'retry-after': '30' });
        // Axios attaches the request config, including the API-key header, to
        // the error. Logging the raw error would publish the credential.
        (failure as Error & { config?: unknown }).config = {
            url: 'https://api.coingecko.com/api/v3/simple/price?ids=stellar',
            headers: { 'x-cg-demo-api-key': SECRET },
        };
        mockedAxios.isAxiosError.mockReturnValue(true);
        mockedAxios.get.mockRejectedValueOnce(failure);

        await expect(provider.fetchPrice('XLM')).rejects.toThrow();

        expect(allLogOutput()).not.toContain(SECRET);
        // The failure is still diagnosable: status and reason are reported.
        expect(mockedLogger.error).toHaveBeenCalledWith(
            'CoinGecko fetch failed for XLM',
            expect.objectContaining({ status: 429 }),
        );
    });

    it('does not leak the key from batch failures', async () => {
        const provider = providerWithKey(SECRET);
        const failure = new Error('boom') as Error & { config?: unknown };
        failure.config = { headers: { 'x-cg-demo-api-key': SECRET } };
        mockedAxios.isAxiosError.mockReturnValue(false);
        mockedAxios.get.mockRejectedValueOnce(failure);

        await expect(provider.fetchPrices(['XLM'])).rejects.toThrow('boom');

        expect(allLogOutput()).not.toContain(SECRET);
    });

    it('logs only whether a key is configured during construction', async () => {
        providerWithKey(SECRET);

        expect(mockedLogger.info).toHaveBeenCalledWith(
            'CoinGecko provider initialized',
            { tier: 'demo', baseUrl: 'https://api.coingecko.com/api/v3', authenticated: true },
        );
        expect(allLogOutput()).not.toContain(SECRET);
    });

    it('reports a free-tier provider as unauthenticated without a key header', async () => {
        const provider = providerWithKey();
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        await provider.fetchPrice('XLM');

        expect(firstRequestArgs().headers).not.toHaveProperty('x-cg-demo-api-key');
        expect(firstRequestArgs().headers).not.toHaveProperty('x-cg-pro-api-key');
    });

    it('routes a whitespace-only key to the free tier instead of the pro host', async () => {
        const provider = providerWithKey('   ');
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        await provider.fetchPrice('XLM');

        expect(mockedLogger.info).toHaveBeenCalledWith(
            'CoinGecko provider initialized',
            expect.objectContaining({ tier: 'free', authenticated: false }),
        );
        expect(firstRequestArgs().headers).not.toHaveProperty('x-cg-demo-api-key');
    });

    it('treats a non-string key as unauthenticated rather than coercing it', async () => {
        const provider = createCoinGeckoProvider(1_234 as unknown as string);
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        await provider.fetchPrice('XLM');

        expect(mockedLogger.info).toHaveBeenCalledWith(
            'CoinGecko provider initialized',
            expect.objectContaining({ tier: 'free', authenticated: false }),
        );
        expect(firstRequestArgs().headers).not.toHaveProperty('x-cg-demo-api-key');
        expect(allLogOutput()).not.toContain('1234');
    });

    it('drops a whitespace key supplied by a directly-constructed config', async () => {
        // Callers may build the provider from their own ProviderConfig instead
        // of using the factory, so the credential is normalized in the
        // constructor too: no blank header may be sent upstream.
        const provider = buildProvider({ maxRequests: 10, windowMs: 60_000 }, '   ');
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        await provider.fetchPrice('XLM');

        expect(mockedLogger.info).toHaveBeenCalledWith(
            'CoinGecko provider initialized',
            expect.objectContaining({ authenticated: false }),
        );
        expect(firstRequestArgs().headers).not.toHaveProperty('x-cg-demo-api-key');
        expect(firstRequestArgs().headers).not.toHaveProperty('x-cg-pro-api-key');
    });

    it('trims a padded key before using it as a credential', async () => {
        const provider = providerWithKey(`  ${SECRET}  `);
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        await provider.fetchPrice('XLM');

        expect(firstRequestArgs().headers['x-cg-demo-api-key']).toBe(SECRET);
    });

    it('routes a non-CG key to the pro host with the pro header', async () => {
        const provider = providerWithKey('live-pro-key');
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        await provider.fetchPrice('XLM');

        expect(firstRequestArgs().url).toContain('pro-api.coingecko.com');
        expect(firstRequestArgs().headers['x-cg-pro-api-key']).toBe('live-pro-key');
    });

    it('keeps the credential out of every axios argument across a mixed batch', async () => {
        const provider = providerWithKey(SECRET);
        replyWith({
            stellar: { usd: 0.15, last_updated_at: T0_SECONDS },
            bitcoin: { usd: 0 },
        });

        await provider.fetchPrices(['XLM', 'BTC']);

        // The key legitimately travels in the header, never in the URL.
        expect(allAxiosArgs()).not.toMatch(/simple\/price[^"]*CG-super/);
    });
});

// ---------------------------------------------------------------------------
// Regression guards for the pre-existing public surface
// ---------------------------------------------------------------------------
describe('public surface remains compatible', () => {
    it('exposes the documented factory defaults', () => {
        const provider = createCoinGeckoProvider();

        expect(provider.name).toBe('coingecko');
        expect(provider.priority).toBe(1);
        expect(provider.weight).toBe(0.6);
        expect(provider.isEnabled).toBe(true);
    });

    it('lists exactly the mapped assets and hands out a fresh array', () => {
        const provider = createCoinGeckoProvider();
        const assets = provider.getSupportedAssets();

        expect(assets).toEqual(['XLM', 'USDC', 'USDT', 'BTC', 'ETH', 'SOL', 'AVAX', 'DOT', 'MATIC', 'LINK']);
        assets.push('BOGUS');
        expect(provider.getSupportedAssets()).not.toContain('BOGUS');
    });

    it('reports an unhealthy provider through healthCheck without leaking the key', async () => {
        const provider = createCoinGeckoProvider('CG-super-secret-demo-key');
        mockedAxios.isAxiosError.mockReturnValue(false);
        mockedAxios.get.mockRejectedValueOnce(httpError(500));

        const health = await provider.healthCheck();

        expect(health.healthy).toBe(false);
        expect(health.error).toContain('500');
        expect(allLogOutput()).not.toContain('CG-super-secret-demo-key');
    });

    it('reports a healthy provider on a successful probe', async () => {
        const provider = createCoinGeckoProvider();
        replyWith({ stellar: { usd: 0.15, last_updated_at: T0_SECONDS } });

        const health = await provider.healthCheck();

        expect(health).toMatchObject({ provider: 'coingecko', healthy: true });
        expect(health.latencyMs).toBeGreaterThanOrEqual(0);
    });

    it('rejects a configuration with a non-positive rate limit', () => {
        expect(() =>
            buildProvider({ maxRequests: 0, windowMs: 60_000 }),
        ).toThrow(RangeError);
        expect(() =>
            buildProvider({ maxRequests: 10, windowMs: 0 }),
        ).toThrow(RangeError);
    });
});
