import axios from 'axios';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { BasePriceProvider } from '../src/providers/base-provider.js';
import type { ProviderConfig, RawPriceData } from '../src/types/index.js';
import { logger } from '../src/utils/logger.js';

vi.mock('axios', () => ({
    default: {
        get: vi.fn(),
    },
}));

class TestProvider extends BasePriceProvider {
    readonly requestedAssets: string[] = [];
    readonly sleeps: number[] = [];
    readonly failures = new Map<string, unknown>();
    failNextSleep = false;

    constructor(rateLimit = { maxRequests: 10, windowMs: 100 }) {
        const config: ProviderConfig = {
            name: 'test-provider',
            enabled: true,
            priority: 1,
            weight: 1,
            baseUrl: 'https://mock.api',
            rateLimit,
        };
        super(config);
    }

    async fetchPrice(asset: string): Promise<RawPriceData> {
        this.requestedAssets.push(asset);
        if (this.failures.has(asset)) {
            throw this.failures.get(asset);
        }

        return {
            asset,
            price: 1,
            timestamp: Math.floor(Date.now() / 1000),
            source: this.name,
        };
    }

    reserveRateLimitSlot(): Promise<void> {
        return this.enforceRateLimit();
    }

    rateLimitState(): { count: number; windowStart: number; lastRequest: number } {
        return {
            count: this.requestCount,
            windowStart: this.windowStartTime,
            lastRequest: this.lastRequestTime,
        };
    }

    makeRequest<T>(url: string, options?: { headers?: Record<string, string> }): Promise<T> {
        return this.request<T>(url, options);
    }

    protected override sleep(ms: number): Promise<void> {
        this.sleeps.push(ms);
        if (this.failNextSleep) {
            this.failNextSleep = false;
            return Promise.reject(new Error('wait failed'));
        }
        return super.sleep(ms);
    }
}

const mockedAxios = vi.mocked(axios);

describe('BasePriceProvider', () => {
    beforeEach(() => {
        vi.useRealTimers();
        vi.clearAllMocks();
    });

    afterEach(() => {
        vi.useRealTimers();
        vi.restoreAllMocks();
    });

    it('treats the cooldown end time as no longer cooled down', () => {
        vi.useFakeTimers();
        vi.setSystemTime(1_000);
        const provider = new TestProvider();
        provider.cooldownUntil = 2_000;

        expect(provider.isCooledDown).toBe(true);
        vi.setSystemTime(2_000);
        expect(provider.isCooledDown).toBe(false);
    });

    it('returns no results for an empty batch and preserves duplicates around partial failures', async () => {
        const provider = new TestProvider();
        provider.failures.set('BAD', new Error('unavailable'));
        const logError = vi.spyOn(logger, 'error').mockImplementation(() => logger);

        await expect(provider.fetchPrices([])).resolves.toEqual([]);
        const results = await provider.fetchPrices(['XLM', 'XLM', 'BAD', 'BTC']);

        expect(results.map(({ asset }) => asset)).toEqual(['XLM', 'XLM', 'BTC']);
        expect(provider.requestedAssets).toEqual(['XLM', 'XLM', 'BAD', 'BTC']);
        expect(logError).toHaveBeenCalledTimes(1);
    });

    it('reports successful health checks and normalizes non-Error failures', async () => {
        const provider = new TestProvider();
        const healthy = await provider.healthCheck();

        expect(healthy).toMatchObject({ provider: 'test-provider', healthy: true });
        expect(healthy.latencyMs).toBeGreaterThanOrEqual(0);

        provider.failures.set('XLM', 'offline');
        const unhealthy = await provider.healthCheck();

        expect(unhealthy).toMatchObject({
            provider: 'test-provider',
            healthy: false,
            error: 'Unknown error',
        });
    });

    it('reserves concurrent requests in order across exact rate-limit window boundaries', async () => {
        vi.useFakeTimers();
        vi.setSystemTime(1_000);
        const provider = new TestProvider({ maxRequests: 1, windowMs: 100 });

        const reservations = Promise.all([
            provider.reserveRateLimitSlot(),
            provider.reserveRateLimitSlot(),
            provider.reserveRateLimitSlot(),
        ]);
        await vi.runAllTimersAsync();
        await reservations;

        expect(provider.sleeps).toEqual([100, 100]);
        expect(provider.rateLimitState()).toEqual({
            count: 1,
            windowStart: 1_200,
            lastRequest: 1_200,
        });
    });

    it('allows later reservations after a rate-limit wait fails', async () => {
        vi.useFakeTimers();
        vi.setSystemTime(1_000);
        const provider = new TestProvider({ maxRequests: 1, windowMs: 100 });
        await provider.reserveRateLimitSlot();
        provider.failNextSleep = true;

        await expect(provider.reserveRateLimitSlot()).rejects.toThrow('wait failed');
        const retry = provider.reserveRateLimitSlot();
        await vi.runAllTimersAsync();
        await retry;

        expect(provider.rateLimitState()).toEqual({
            count: 1,
            windowStart: 1_100,
            lastRequest: 1_100,
        });
    });

    it('forwards request headers and propagates HTTP failures', async () => {
        const provider = new TestProvider();
        mockedAxios.get.mockResolvedValueOnce({ data: { ok: true } } as never);
        await expect(
            provider.makeRequest('https://mock.api/prices', {
                headers: { Authorization: 'Bearer test-token' },
            }),
        ).resolves.toEqual({ ok: true });
        expect(mockedAxios.get).toHaveBeenCalledWith(
            'https://mock.api/prices',
            expect.objectContaining({
                headers: {
                    'Content-Type': 'application/json',
                    Authorization: 'Bearer test-token',
                },
                timeout: 30_000,
            }),
        );

        const failure = new Error('network unavailable');
        mockedAxios.get.mockRejectedValueOnce(failure);
        await expect(provider.makeRequest('https://mock.api/prices')).rejects.toBe(failure);
    });
});
