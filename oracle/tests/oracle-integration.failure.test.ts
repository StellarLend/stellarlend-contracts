/**
 * Failure-path and boundary coverage for the Oracle integration service.
 *
 * These tests exercise the public OracleService contract under adverse
 * conditions: malformed/invalid input, exact boundary values, duplicate and
 * concurrent requests, provider retries/partial failures, unauthorized admin
 * configuration, and failure observability. All dependencies are mocked so the
 * suite is deterministic and never touches the network or a chain.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { OracleService } from '../src/index.js';
import type { OracleServiceConfig } from '../src/config.js';

/**
 * Hoisted mutable controller shared between the module mocks and the tests.
 * `vi.hoisted` runs before the `vi.mock` factories are evaluated.
 */
const h = vi.hoisted(() => {
    const state = {
        cgCalls: 0,
        bnCalls: 0,
        updaterCalls: 0,
        updaterShouldThrow: false,
        cgImpl: (_asset: string) => ({
            asset: _asset,
            price: 0.5,
            timestamp: Math.floor(Date.now() / 1000),
            source: 'coingecko',
        }),
        bnImpl: (_asset: string) => ({
            asset: _asset,
            price: 0.5,
            timestamp: Math.floor(Date.now() / 1000),
            source: 'binance',
        }),
    };
    return state;
});

vi.mock('../src/services/contract-updater.js', () => ({
    createContractUpdater: vi.fn(() => ({
        updatePrices: vi.fn(async (prices: Array<{ asset: string; price: bigint; timestamp: number }>) => {
            h.updaterCalls++;
            if (h.updaterShouldThrow) {
                throw new Error('chain unavailable');
            }
            return prices.map((p) => ({
                success: true,
                asset: p.asset,
                price: p.price,
                timestamp: p.timestamp,
            }));
        }),
        healthCheck: vi.fn().mockResolvedValue(true),
        getAdminPublicKey: vi.fn().mockReturnValue('GTEST123'),
    })),
    ContractUpdater: vi.fn(),
}));

vi.mock('../src/providers/coingecko.js', () => ({
    createCoinGeckoProvider: vi.fn(() => ({
        name: 'coingecko',
        isEnabled: true,
        priority: 1,
        weight: 0.6,
        getSupportedAssets: () => ['XLM', 'BTC', 'ETH'],
        fetchPrice: vi.fn(async (asset: string) => {
            h.cgCalls++;
            return h.cgImpl(asset);
        }),
    })),
}));

vi.mock('../src/providers/binance.js', () => ({
    createBinanceProvider: vi.fn(() => ({
        name: 'binance',
        isEnabled: true,
        priority: 2,
        weight: 0.4,
        getSupportedAssets: () => ['XLM', 'BTC', 'ETH'],
        fetchPrice: vi.fn(async (asset: string) => {
            h.bnCalls++;
            return h.bnImpl(asset);
        }),
    })),
}));

/**
 * Build a valid base configuration. Only safe, non-secret bounds are used and
 * the admin secret is a clearly fake value so leaks are easy to detect.
 */
function baseConfig(overrides: Partial<OracleServiceConfig> = {}): OracleServiceConfig {
    return {
        stellarNetwork: 'testnet',
        stellarRpcUrl: 'https://soroban-testnet.stellar.org',
        contractId: 'CTEST123',
        adminSecretKey: 'S-FAKE-ADMIN-SECRET-DO-NOT-LEAK',
        updateIntervalMs: 1000,
        maxPriceDeviationPercent: 10,
        madZScoreThreshold: 3.5,
        priceStaleThresholdSeconds: 300,
        cacheTtlSeconds: 30,
        logLevel: 'error',
        providers: [
            {
                name: 'coingecko',
                enabled: true,
                priority: 1,
                weight: 0.6,
                baseUrl: 'https://api.coingecko.com/api/v3',
                rateLimit: { maxRequests: 10, windowMs: 60000 },
            },
            {
                name: 'binance',
                enabled: true,
                priority: 2,
                weight: 0.4,
                baseUrl: 'https://api.binance.com/api/v3',
                rateLimit: { maxRequests: 1200, windowMs: 60000 },
            },
        ],
        priceBounds: {
            XLM: { minPrice: 0.1, maxPrice: 1 },
            BTC: { minPrice: 1, maxPrice: 100000 },
            ETH: { minPrice: 1, maxPrice: 10000 },
            USDC: { minPrice: 0.0001, maxPrice: 2 },
            USDT: { minPrice: 0.0001, maxPrice: 2 },
        },
        ...overrides,
    };
}

/** Both providers return the same valid price for the requested asset. */
function bothHealthy(price = 0.5): void {
    h.cgImpl = (asset: string) => ({
        asset,
        price,
        timestamp: Math.floor(Date.now() / 1000),
        source: 'coingecko',
    });
    h.bnImpl = (asset: string) => ({
        asset,
        price,
        timestamp: Math.floor(Date.now() / 1000),
        source: 'binance',
    });
}

describe('OracleService failure paths and boundaries', () => {
    let service: OracleService | undefined;

    beforeEach(() => {
        h.cgCalls = 0;
        h.bnCalls = 0;
        h.updaterCalls = 0;
        h.updaterShouldThrow = false;
        bothHealthy();
    });

    afterEach(() => {
        if (service) {
            service.stop();
            service = undefined;
        }
        vi.useRealTimers();
    });

    describe('valid input (regression)', () => {
        it('returns a scaled bigint price with source metadata', async () => {
            service = new OracleService(baseConfig());

            const price = await service.fetchPrice('XLM');

            expect(price).not.toBeNull();
            expect(price?.asset).toBe('XLM');
            expect(typeof price?.price).toBe('bigint');
            expect(price!.price).toBeGreaterThan(0n);
            expect(price?.confidence).toBeGreaterThanOrEqual(0);
            expect(price?.confidence).toBeLessThanOrEqual(100);
        });

        it('submits aggregated prices to the contract updater exactly once', async () => {
            service = new OracleService(baseConfig());

            await service.updatePrices(['XLM', 'BTC', 'ETH']);

            expect(h.updaterCalls).toBe(1);
        });
    });

    describe('malformed and invalid input is rejected', () => {
        it.each([
            ['zero', 0],
            ['negative', -0.5],
            ['NaN', Number.NaN],
            ['Infinity', Number.POSITIVE_INFINITY],
            ['below asset minimum', 0.05],
            ['above asset maximum', 5],
        ])('rejects a %s price and does not submit', async (_label, badPrice) => {
            h.cgImpl = (asset) => ({
                asset,
                price: badPrice as number,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'coingecko',
            });
            h.bnImpl = (asset) => ({
                asset,
                price: badPrice as number,
                timestamp: Math.floor(Date.now() / 1000),
                source: 'binance',
            });

            service = new OracleService(baseConfig());

            expect(await service.fetchPrice('XLM')).toBeNull();
            await service.updatePrices(['XLM']);
            expect(h.updaterCalls).toBe(0);
        });

        it('rejects a stale price older than the staleness threshold', async () => {
            h.cgImpl = (asset) => ({
                asset,
                price: 0.5,
                timestamp: Math.floor(Date.now() / 1000) - 301,
                source: 'coingecko',
            });
            h.bnImpl = (asset) => ({
                asset,
                price: 0.5,
                timestamp: Math.floor(Date.now() / 1000) - 301,
                source: 'binance',
            });

            service = new OracleService(baseConfig());

            expect(await service.fetchPrice('XLM')).toBeNull();
        });

        it('rejects a price whose timestamp is in the future', async () => {
            const future = Math.floor(Date.now() / 1000) + 3600;
            h.cgImpl = (asset) => ({ asset, price: 0.5, timestamp: future, source: 'coingecko' });
            h.bnImpl = (asset) => ({ asset, price: 0.5, timestamp: future, source: 'binance' });

            service = new OracleService(baseConfig());

            expect(await service.fetchPrice('XLM')).toBeNull();
        });

        it('handles an empty asset list without submitting', async () => {
            service = new OracleService(baseConfig());

            await expect(service.updatePrices([])).resolves.toBeUndefined();
            expect(h.updaterCalls).toBe(0);
        });
    });

    describe('boundary values', () => {
        it('accepts a price exactly at the asset minimum', async () => {
            bothHealthy(0.1);
            service = new OracleService(baseConfig());

            expect(await service.fetchPrice('XLM')).not.toBeNull();
        });

        it('accepts a price exactly at the asset maximum', async () => {
            bothHealthy(1);
            service = new OracleService(baseConfig());

            expect(await service.fetchPrice('XLM')).not.toBeNull();
        });

        it('rejects a price just below the asset minimum', async () => {
            bothHealthy(0.0999);
            service = new OracleService(baseConfig());

            expect(await service.fetchPrice('XLM')).toBeNull();
        });

        it('rejects a price just above the asset maximum', async () => {
            bothHealthy(1.0001);
            service = new OracleService(baseConfig());

            expect(await service.fetchPrice('XLM')).toBeNull();
        });

        it('accepts a timestamp exactly at the staleness threshold', async () => {
            vi.useFakeTimers();
            vi.setSystemTime(new Date('2026-01-01T00:00:00.000Z'));
            const nowSec = Math.floor(Date.now() / 1000);
            h.cgImpl = (asset) => ({
                asset,
                price: 0.5,
                timestamp: nowSec - 300,
                source: 'coingecko',
            });
            h.bnImpl = (asset) => ({
                asset,
                price: 0.5,
                timestamp: nowSec - 300,
                source: 'binance',
            });

            service = new OracleService(baseConfig());

            expect(await service.fetchPrice('XLM')).not.toBeNull();
        });

        it('rejects a timestamp one second past the staleness threshold', async () => {
            vi.useFakeTimers();
            vi.setSystemTime(new Date('2026-01-01T00:00:00.000Z'));
            const nowSec = Math.floor(Date.now() / 1000);
            h.cgImpl = (asset) => ({
                asset,
                price: 0.5,
                timestamp: nowSec - 301,
                source: 'coingecko',
            });
            h.bnImpl = (asset) => ({
                asset,
                price: 0.5,
                timestamp: nowSec - 301,
                source: 'binance',
            });

            service = new OracleService(baseConfig());

            expect(await service.fetchPrice('XLM')).toBeNull();
        });

        it('rejects a price whose scaled value exceeds the safe integer range', async () => {
            // Large finite value that is below no max bound only when bounds are
            // widened; explicitly widen bounds to isolate the scaling guard.
            bothHealthy(Number.MAX_SAFE_INTEGER);
            service = new OracleService(
                baseConfig({
                    priceBounds: {
                        XLM: { minPrice: 0.0001, maxPrice: Number.MAX_SAFE_INTEGER },
                        BTC: { minPrice: 1, maxPrice: 100000 },
                        ETH: { minPrice: 1, maxPrice: 10000 },
                        USDC: { minPrice: 0.0001, maxPrice: 2 },
                        USDT: { minPrice: 0.0001, maxPrice: 2 },
                    },
                }),
            );

            expect(await service.fetchPrice('XLM')).toBeNull();
        });
    });

    describe('duplicate and concurrent requests', () => {
        it('coalesces concurrent identical fetches into a single provider call', async () => {
            service = new OracleService(baseConfig());

            const results = await Promise.all(
                Array.from({ length: 10 }, () => service!.fetchPrice('XLM')),
            );

            expect(h.cgCalls).toBe(1);
            results.forEach((r) => expect(r).not.toBeNull());
            expect(new Set(results.map((r) => r!.price)).size).toBe(1);
        });

        it('fetches different assets independently under concurrency', async () => {
            // 1 is valid for XLM (max), BTC (min) and ETH (min).
            bothHealthy(1);
            service = new OracleService(baseConfig());

            const results = await Promise.all([
                service.fetchPrice('XLM'),
                service.fetchPrice('BTC'),
                service.fetchPrice('ETH'),
            ]);

            expect(results).toHaveLength(3);
            expect(results.map((r) => r?.asset)).toEqual(['XLM', 'BTC', 'ETH']);
        });

        it('keeps concurrent update cycles from throwing', async () => {
            service = new OracleService(baseConfig());

            await expect(
                Promise.all([service.updatePrices(['XLM']), service.updatePrices(['BTC'])]),
            ).resolves.not.toThrow();
            expect(h.updaterCalls).toBeGreaterThanOrEqual(1);
        });
    });

    describe('retry and partial failure', () => {
        it('retries a transient provider failure then succeeds', async () => {
            vi.useFakeTimers();
            vi.setSystemTime(new Date('2026-01-01T00:00:00.000Z'));

            h.cgImpl = (asset) => {
                if (h.cgCalls < 3) {
                    throw new Error('transient provider error');
                }
                return {
                    asset,
                    price: 0.5,
                    timestamp: Math.floor(Date.now() / 1000),
                    source: 'coingecko',
                };
            };
            h.bnImpl = () => {
                throw new Error('binance down');
            };

            service = new OracleService(baseConfig());

            const pending = service.fetchPrice('XLM');
            await vi.advanceTimersByTimeAsync(1000);
            const result = await pending;

            // providerRetries default is 2 => 3 attempts before success.
            expect(h.cgCalls).toBe(3);
            expect(result).not.toBeNull();
            expect(result?.asset).toBe('XLM');
        });

        it('returns a usable price when one provider fails and the other works', async () => {
            h.cgImpl = () => {
                throw new Error('coingecko down');
            };
            bothHealthyForBinance(0.5, 'binance');

            service = new OracleService(baseConfig());

            const price = await service.fetchPrice('XLM');

            expect(price).not.toBeNull();
            expect(price?.asset).toBe('XLM');
        });

        it('recovers once a previously failing provider becomes healthy', async () => {
            h.cgImpl = () => {
                throw new Error('coingecko down');
            };
            bothHealthyForBinance(1, 'binance');

            const failing = new OracleService(baseConfig());
            try {
                const first = await failing.fetchPrice('BTC');
                expect(first).not.toBeNull();
            } finally {
                failing.stop();
            }

            // New service simulates the dependency having recovered.
            bothHealthy(1);
            service = new OracleService(baseConfig());
            const second = await service.fetchPrice('BTC');
            expect(second).not.toBeNull();
        });
    });

    describe('unauthorized admin configuration', () => {
        it('rejects an admin API port without an HMAC secret', () => {
            expect(() => new OracleService(baseConfig({ adminApiPort: 9123 }))).toThrow(
                /ADMIN_HMAC_SECRET/,
            );
        });

        it('does not leak the admin secret in the thrown error', () => {
            const secret = 'S-FAKE-ADMIN-SECRET-DO-NOT-LEAK';
            try {
                new OracleService(baseConfig({ adminApiPort: 9123, adminSecretKey: secret }));
                throw new Error('expected constructor to throw');
            } catch (error) {
                const message = error instanceof Error ? error.message : String(error);
                expect(message).not.toContain(secret);
            }
        });

        it('keeps the admin HMAC secret out of the public status', () => {
            service = new OracleService(
                baseConfig({ adminApiPort: 9124, adminHmacSecret: 'hmac-secret-value' }),
            );

            const serialized = JSON.stringify(service.getStatus());

            expect(serialized).not.toContain('hmac-secret-value');
            expect(serialized).not.toContain('S-FAKE-ADMIN-SECRET-DO-NOT-LEAK');
        });
    });

    describe('failure observability and state transitions', () => {
        it('absorbs contract updater failures without rejecting the update cycle', async () => {
            h.updaterShouldThrow = true;
            service = new OracleService(baseConfig());

            await expect(service.updatePrices(['XLM'])).resolves.toBeUndefined();
            expect(service.getStatus().isRunning).toBe(false);
        });

        it('tolerates provider failures without throwing from an update cycle', async () => {
            h.cgImpl = () => {
                throw new Error('coingecko down');
            };
            h.bnImpl = () => {
                throw new Error('binance down');
            };
            service = new OracleService(baseConfig());

            await expect(service.updatePrices(['XLM'])).resolves.not.toThrow();
            expect(h.updaterCalls).toBe(0);
        });

        it('reports a stable status shape across start/stop transitions', async () => {
            service = new OracleService(baseConfig());

            expect(service.getStatus().isRunning).toBe(false);
            await service.start(['XLM']);
            expect(service.getStatus().isRunning).toBe(true);

            service.stop();
            expect(service.getStatus().isRunning).toBe(false);

            const status = service.getStatus();
            expect(status.network).toBe('testnet');
            expect(status.contractId).toBe('CTEST123');
            expect(Array.isArray(status.providers)).toBe(true);
            expect(status.providers.length).toBeGreaterThan(0);
            expect(status.aggregatorStats).toBeDefined();
        });

        it('ignores a second start call while already running', async () => {
            service = new OracleService(baseConfig());

            await service.start(['XLM']);
            await service.start(['XLM']);

            expect(service.getStatus().isRunning).toBe(true);
        });
    });
});

/** Helper: only binance is healthy and returns a valid price for any asset. */
function bothHealthyForBinance(price: number, source: string): void {
    h.bnImpl = (asset) => ({
        asset,
        price,
        timestamp: Math.floor(Date.now() / 1000),
        source,
    });
}
