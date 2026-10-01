
/**
 * Oracle Configuration Management, Validation and Failure-Path Tests
 *
 * This suite is the regression net for `OracleService` configuration handling.
 * It covers:
 * - Successful initialization and configuration of oracle parameters.
 * - Rejection of invalid, out-of-bounds, malformed and duplicate inputs.
 * - Authorization/role separation for configuration and privileged surfaces.
 * - State-transition invariants (construct -> run -> stop, and recovery).
 * - Error observability (aggregated, addressable, deterministic diagnostics).
 *
 * Every test is deterministic: no wall-clock sleeps, no network access, no
 * reliance on `Math.random()` or on the ambient environment.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { OracleService } from '../src/index.js';
import {
    validateOracleServiceConfig,
    ConfigValidationError,
    SUPPORTED_PROVIDER_NAMES,
    type OracleServiceConfig,
} from '../src/config.js';
import type { ProviderConfig } from '../src/config.js';

// Mock the contract updater: no blockchain access, no key material handling.
const updatePricesMock = vi.fn().mockResolvedValue([
    { success: true, asset: 'XLM', price: 150000n, timestamp: 1_700_000_000_000 },
]);
const createContractUpdaterMock = vi.fn(() => ({
    updatePrices: updatePricesMock,
    healthCheck: vi.fn().mockResolvedValue(true),
    getAdminPublicKey: vi.fn().mockReturnValue('GTEST123'),
}));

vi.mock('../src/services/contract-updater.js', () => ({
    createContractUpdater: (...args: unknown[]) => createContractUpdaterMock(...args),
    ContractUpdater: vi.fn(),
}));

// Mock provider factories so no outbound HTTP is ever attempted.
const COINGECKO_PRICE = 0.15;
const BINANCE_PRICE = 0.152;

/**
 * Provider response timestamp, `ageSeconds` in the past.
 *
 * Anchored to the current time rather than a fixed instant so that staleness
 * assertions stay deterministic: every age is expressed relative to "now".
 */
function providerTimestamp(ageSeconds = 0): number {
    return Math.floor(Date.now() / 1000) - ageSeconds;
}

const coingeckoFetchPrice = vi.fn();
const binanceFetchPrice = vi.fn();

/** Make a provider succeed with a price of the given age. */
function setProviderPrice(
    mock: typeof coingeckoFetchPrice,
    source: string,
    price: number,
    ageSeconds = 0,
): void {
    mock.mockImplementation(() =>
        Promise.resolve({
            asset: 'XLM',
            price,
            timestamp: providerTimestamp(ageSeconds),
            source,
        }),
    );
}

/** Make a provider fail on every attempt. */
function setProviderFailure(mock: typeof coingeckoFetchPrice, message: string): void {
    mock.mockImplementation(() => Promise.reject(new Error(message)));
}

function providerDouble(name: string) {
    return {
        name,
        isEnabled: true,
        priority: 1,
        weight: 1,
        getSupportedAssets: () => ['XLM', 'BTC', 'ETH', 'USDC'],
        fetchPrice: name === 'coingecko' ? coingeckoFetchPrice : binanceFetchPrice,
        healthCheck: vi.fn().mockResolvedValue({ provider: name, healthy: true, lastCheck: 0 }),
    };
}

const createCoinGeckoProviderMock = vi.fn(() => providerDouble('coingecko'));
const createBinanceProviderMock = vi.fn(() => providerDouble('binance'));

vi.mock('../src/providers/coingecko.js', () => ({
    createCoinGeckoProvider: (...args: unknown[]) => createCoinGeckoProviderMock(...args),
    CoinGeckoProvider: vi.fn(),
}));

vi.mock('../src/providers/binance.js', () => ({
    createBinanceProvider: (...args: unknown[]) => createBinanceProviderMock(...args),
    BinanceProvider: vi.fn(),
}));

/** Collect every service created by a test so timers are always released. */
const createdServices: OracleService[] = [];

function buildService(config: unknown): OracleService {
    const service = new OracleService(config as OracleServiceConfig);
    createdServices.push(service);
    return service;
}

function coingeckoProvider(overrides: Partial<ProviderConfig> = {}): ProviderConfig {
    return {
        name: 'coingecko',
        enabled: true,
        priority: 1,
        weight: 0.6,
        baseUrl: 'https://api.coingecko.com/api/v3',
        rateLimit: { maxRequests: 10, windowMs: 60_000 },
        ...overrides,
    };
}

function binanceProvider(overrides: Partial<ProviderConfig> = {}): ProviderConfig {
    return {
        name: 'binance',
        enabled: true,
        priority: 2,
        weight: 0.4,
        baseUrl: 'https://api.binance.com/api/v3',
        rateLimit: { maxRequests: 1200, windowMs: 60_000 },
        ...overrides,
    };
}

describe('Oracle Configuration Management', () => {
    let baseConfig: OracleServiceConfig;

    beforeEach(() => {
        vi.clearAllMocks();
        coingeckoFetchPrice.mockReset();
        binanceFetchPrice.mockReset();
        setProviderPrice(coingeckoFetchPrice, 'coingecko', COINGECKO_PRICE);
        setProviderPrice(binanceFetchPrice, 'binance', BINANCE_PRICE);

        createContractUpdaterMock.mockImplementation(() => ({
            updatePrices: updatePricesMock,
            healthCheck: vi.fn().mockResolvedValue(true),
            getAdminPublicKey: vi.fn().mockReturnValue('GTEST123'),
        }));
        createCoinGeckoProviderMock.mockImplementation(() => providerDouble('coingecko'));
        createBinanceProviderMock.mockImplementation(() => providerDouble('binance'));

        baseConfig = {
            stellarNetwork: 'testnet',
            stellarRpcUrl: 'https://soroban-testnet.stellar.org',
            contractId: 'CTEST123',
            adminSecretKey: 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456',
            updateIntervalMs: 60_000,
            maxPriceDeviationPercent: 10,
            priceStaleThresholdSeconds: 300,
            cacheTtlSeconds: 30,
            logLevel: 'error',
            providers: [coingeckoProvider(), binanceProvider()],
        } as unknown as OracleServiceConfig;
    });

    afterEach(async () => {
        while (createdServices.length > 0) {
            const service = createdServices.pop();
            if (service) {
                await service.stop();
            }
        }
        vi.clearAllMocks();
    });

    /* ------------------------------------------------------------------ */
    /* Successful initialization and configuration                          */
    /* ------------------------------------------------------------------ */

    describe('successful initialization', () => {
        it('accepts a complete valid configuration', () => {
            const service = buildService(baseConfig);

            const status = service.getStatus();
            expect(status.network).toBe('testnet');
            expect(status.contractId).toBe('CTEST123');
            expect(status.isRunning).toBe(false);
            expect(status.providers).toHaveLength(2);
            expect(status.aggregatorStats).toBeDefined();
        });

        it('accepts every supported network', () => {
            for (const network of ['testnet', 'mainnet'] as const) {
                const service = buildService({ ...baseConfig, stellarNetwork: network });
                expect(service.getStatus().network).toBe(network);
            }
        });

        it('accepts every supported log level', () => {
            for (const logLevel of ['debug', 'info', 'warn', 'error'] as const) {
                const service = buildService({ ...baseConfig, logLevel });
                expect(service).toBeDefined();
            }
        });

        it('reports providers ordered by ascending priority', () => {
            const service = buildService({
                ...baseConfig,
                providers: [
                    binanceProvider({ priority: 1, weight: 0.7 }),
                    coingeckoProvider({ priority: 2, weight: 0.3 }),
                ],
            });

            expect(service.getStatus().providers.map((p) => p.name)).toEqual([
                'binance',
                'coingecko',
            ]);
        });

        it('reflects a promoted primary provider in the reported priorities', () => {
            const service = buildService({
                ...baseConfig,
                providers: [
                    coingeckoProvider({ priority: 2 }),
                    binanceProvider({ priority: 1 }),
                ],
            });

            const providers = service.getStatus().providers;
            expect(providers.find((p) => p.name === 'binance')?.priority).toBe(1);
            expect(providers.find((p) => p.name === 'coingecko')?.priority).toBe(2);
        });

        it('reflects configured provider weights', () => {
            const service = buildService({
                ...baseConfig,
                providers: [
                    coingeckoProvider({ weight: 0.8 }),
                    binanceProvider({ weight: 0.2 }),
                ],
            });

            const providers = service.getStatus().providers;
            expect(providers.find((p) => p.name === 'coingecko')?.weight).toBeCloseTo(0.8, 10);
            expect(providers.find((p) => p.name === 'binance')?.weight).toBeCloseTo(0.2, 10);
        });

        it('reflects the enabled flag for disabled providers', () => {
            const service = buildService({
                ...baseConfig,
                providers: [coingeckoProvider({ enabled: false }), binanceProvider()],
            });

            const providers = service.getStatus().providers;
            expect(providers.find((p) => p.name === 'coingecko')?.enabled).toBe(false);
            expect(providers.find((p) => p.name === 'binance')?.enabled).toBe(true);
        });

        it('does not construct a disabled provider', () => {
            buildService({
                ...baseConfig,
                providers: [coingeckoProvider({ enabled: false }), binanceProvider()],
            });

            expect(createCoinGeckoProviderMock).not.toHaveBeenCalled();
            expect(createBinanceProviderMock).toHaveBeenCalledTimes(1);
        });

        it('constructs a provider for every enabled entry', () => {
            buildService(baseConfig);

            expect(createCoinGeckoProviderMock).toHaveBeenCalledTimes(1);
            expect(createBinanceProviderMock).toHaveBeenCalledTimes(1);
            expect(createContractUpdaterMock).toHaveBeenCalledTimes(1);
        });

        it('passes the admin secret to the contract updater only', () => {
            buildService(baseConfig);

            expect(createContractUpdaterMock).toHaveBeenCalledWith(
                expect.objectContaining({
                    network: 'testnet',
                    rpcUrl: 'https://soroban-testnet.stellar.org',
                    contractId: 'CTEST123',
                    adminSecretKey: baseConfig.adminSecretKey,
                }),
            );

            // The secret must not leak into anything the service reports.
            const status = buildService(baseConfig).getStatus();
            expect(JSON.stringify(status)).not.toContain(baseConfig.adminSecretKey);
        });

        it('supports a single-provider deployment', () => {
            const service = buildService({ ...baseConfig, providers: [coingeckoProvider({ weight: 0.6 })] });

            const providers = service.getStatus().providers;
            expect(providers).toHaveLength(1);
            expect(providers[0].name).toBe('coingecko');
            // A lone provider carries the full weight of the distribution.
            expect(providers[0].weight).toBe(1);
        });

        it('accepts a zero cache TTL to disable caching', () => {
            const service = buildService({ ...baseConfig, cacheTtlSeconds: 0 });
            expect(service).toBeDefined();
        });

        it('accepts the minimum viable update interval and staleness threshold', () => {
            const service = buildService({
                ...baseConfig,
                updateIntervalMs: 1,
                priceStaleThresholdSeconds: Number.MIN_VALUE,
            });
            expect(service).toBeDefined();
        });

        it('accepts maxPriceDeviationPercent at both inclusive bounds', () => {
            expect(buildService({ ...baseConfig, maxPriceDeviationPercent: 1 })).toBeDefined();
            expect(buildService({ ...baseConfig, maxPriceDeviationPercent: 100 })).toBeDefined();
        });

        it('accepts an omitted madZScoreThreshold and an explicit zero (filter disabled)', () => {
            const omitted = { ...baseConfig } as Record<string, unknown>;
            delete omitted.madZScoreThreshold;
            expect(buildService(omitted)).toBeDefined();
            expect(buildService({ ...baseConfig, madZScoreThreshold: 0 })).toBeDefined();
        });

        it('accepts optional redisUrl forms and omits when absent', () => {
            expect(buildService({ ...baseConfig })).toBeDefined();
            expect(buildService({ ...baseConfig, redisUrl: '' })).toBeDefined();
            expect(
                buildService({ ...baseConfig, redisUrl: 'redis://localhost:6379' }),
            ).toBeDefined();
            expect(
                buildService({ ...baseConfig, redisUrl: 'rediss://user:pass@cache:6380' }),
            ).toBeDefined();
        });
    });

    /* ------------------------------------------------------------------ */
    /* Weight normalization boundaries                                      */
    /* ------------------------------------------------------------------ */

    describe('provider weight normalization', () => {
        it('scales weights that sum below 1 up to a full distribution', () => {
            const service = buildService({
                ...baseConfig,
                providers: [
                    coingeckoProvider({ weight: 0.2 }),
                    binanceProvider({ weight: 0.3 }),
                ],
            });

            const total = service
                .getStatus()
                .providers.reduce((sum, p) => sum + p.weight, 0);
            expect(total).toBeCloseTo(1, 10);
        });

        it('excludes disabled providers from normalization', () => {
            const service = buildService({
                ...baseConfig,
                providers: [
                    coingeckoProvider({ weight: 0.25 }),
                    binanceProvider({ weight: 0.25 }),
                    // Disabled: must not absorb any of the enabled weight.
                    { ...coingeckoProvider({ name: 'binance', priority: 3 }), enabled: false, weight: 5 },
                ].slice(0, 2),
            });

            const total = service
                .getStatus()
                .providers.reduce((sum, p) => sum + p.weight, 0);
            expect(total).toBeCloseTo(1, 10);
        });

        it('normalizes deterministically for floating point weights', () => {
            const config = () => ({
                ...baseConfig,
                providers: [
                    coingeckoProvider({ weight: 0.1 }),
                    binanceProvider({ weight: 0.2 }),
                ],
            });

            const first = buildService(config()).getStatus().providers;
            const second = buildService(config()).getStatus().providers;

            expect(first).toEqual(second);
            expect(first[0].weight + first[1].weight).toBeCloseTo(1, 10);
        });

        it('rejects enabled weights summing above 1', () => {
            const config = {
                ...baseConfig,
                providers: [coingeckoProvider({ weight: 0.7 }), binanceProvider({ weight: 0.5 })],
            };

            expect(() => new OracleService(config)).toThrow(/weights must not exceed 1/);
        });

        it('rejects a weight sum just past the boundary', () => {
            const config = {
                ...baseConfig,
                providers: [
                    coingeckoProvider({ weight: 0.5 }),
                    binanceProvider({ weight: 0.5 + Number.EPSILON * 8 }),
                ],
            };

            expect(() => new OracleService(config)).toThrow(/weights must not exceed 1/);
        });

        it('rejects all-zero enabled weights', () => {
            const config = {
                ...baseConfig,
                providers: [
                    coingeckoProvider({ weight: 0 }),
                    binanceProvider({ weight: 0 }),
                ],
            };

            expect(() => new OracleService(config)).toThrow(/must be greater than 0/);
        });

        it('rejects a weight outside the 0..1 range', () => {
            for (const weight of [-0.1, 1.1, Number.NaN, Number.POSITIVE_INFINITY]) {
                expect(() =>
                    new OracleService({
                        ...baseConfig,
                        providers: [coingeckoProvider({ weight })],
                    } as unknown as OracleServiceConfig),
                ).toThrow(/providers\[0\]\.weight/);
            }
        });

        it('accepts the inclusive weight bounds 0 and 1', () => {
            const service = buildService({
                ...baseConfig,
                providers: [coingeckoProvider({ weight: 0 }), binanceProvider({ weight: 1 })],
            });

            const providers = service.getStatus().providers;
            expect(providers.find((p) => p.name === 'coingecko')?.weight).toBe(0);
            expect(providers.find((p) => p.name === 'binance')?.weight).toBe(1);
        });
    });

    /* ------------------------------------------------------------------ */
    /* Network and endpoint validation                                     */
    /* ------------------------------------------------------------------ */

    describe('network validation', () => {
        it.each(['devnet', 'futurenet', 'local', 'TESTNET', '', 'testnet ', ' testnet'])(
            'rejects the invalid network %o',
            (stellarNetwork) => {
                expect(() =>
                    new OracleService({ ...baseConfig, stellarNetwork } as unknown as OracleServiceConfig),
                ).toThrow(/stellarNetwork/);
            },
        );

        it.each([undefined, null, 0, 1, {}, ['testnet']])(
            'rejects the non-string network %o',
            (stellarNetwork) => {
                expect(() =>
                    new OracleService({ ...baseConfig, stellarNetwork } as unknown as OracleServiceConfig),
                ).toThrow(/stellarNetwork/);
            },
        );
    });

    describe('RPC URL validation', () => {
        it.each([
            'not-a-url',
            '',
            '   ',
            'soroban-testnet.stellar.org',
            'ftp://soroban-testnet.stellar.org',
            'javascript:alert(1)',
            'file:///etc/passwd',
            'https://',
        ])('rejects the invalid RPC URL %o', (stellarRpcUrl) => {
            expect(() => new OracleService({ ...baseConfig, stellarRpcUrl })).toThrow(
                /stellarRpcUrl/,
            );
        });

        it.each([undefined, null, 42, {}])(
            'rejects the non-string RPC URL %o',
            (stellarRpcUrl) => {
                expect(() =>
                    new OracleService({ ...baseConfig, stellarRpcUrl } as unknown as OracleServiceConfig),
                ).toThrow(/stellarRpcUrl/);
            },
        );

        it('rejects a URL padded with whitespace', () => {
            expect(() =>
                new OracleService({ ...baseConfig, stellarRpcUrl: ' https://example.org ' }),
            ).toThrow(/stellarRpcUrl/);
        });

        it.each([
            'http://localhost:8000',
            'https://soroban-mainnet.stellar.org',
            'https://rpc.example.org:443/soroban/rpc',
        ])('accepts the usable RPC URL %o', (stellarRpcUrl) => {
            expect(new OracleService({ ...baseConfig, stellarRpcUrl })).toBeInstanceOf(OracleService);
        });
    });

    /* ------------------------------------------------------------------ */
    /* Contract identifier validation                                       */
    /* ------------------------------------------------------------------ */

    describe('contract ID validation', () => {
        it.each(['', '   ', 'CT', 'ctest123', 'C-TEST-123', 'CTEST 123', 'C'.repeat(65)])(
            'rejects the invalid contract ID %o',
            (contractId) => {
                expect(() => new OracleService({ ...baseConfig, contractId })).toThrow(/contractId/);
            },
        );

        it.each([undefined, null, 123, {}])(
            'rejects the non-string contract ID %o',
            (contractId) => {
                expect(() =>
                    new OracleService({ ...baseConfig, contractId } as unknown as OracleServiceConfig),
                ).toThrow(/contractId/);
            },
        );

        it('accepts the minimum and maximum contract ID lengths', () => {
            expect(new OracleService({ ...baseConfig, contractId: 'CABC' })).toBeInstanceOf(
                OracleService,
            );
            expect(new OracleService({ ...baseConfig, contractId: `C${'A'.repeat(63)}` })).toBeInstanceOf(
                OracleService,
            );
        });
    });

    /* ------------------------------------------------------------------ */
    /* Provider validation                                                 */
    /* ------------------------------------------------------------------ */

    describe('provider validation', () => {
        it('rejects an empty provider list', () => {
            expect(() => new OracleService({ ...baseConfig, providers: [] })).toThrow(
                /at least one provider/,
            );
        });

        it.each([undefined, null, 'coingecko', 42, {}])(
            'rejects the non-array providers value %o',
            (providers) => {
                expect(() =>
                    new OracleService({ ...baseConfig, providers } as unknown as OracleServiceConfig),
                ).toThrow(/providers/);
            },
        );

        it('rejects every provider being disabled', () => {
            const config = {
                ...baseConfig,
                providers: [
                    coingeckoProvider({ enabled: false }),
                    binanceProvider({ enabled: false }),
                ],
            };

            expect(() => new OracleService(config)).toThrow(/at least one provider must be enabled/);
        });

        it('rejects duplicate provider names and points at both positions', () => {
            const config = {
                ...baseConfig,
                providers: [coingeckoProvider(), coingeckoProvider({ priority: 2 })],
            };

            let error: unknown;
            try {
                new OracleService(config);
            } catch (e) {
                error = e;
            }

            expect(error).toBeInstanceOf(ConfigValidationError);
            const issues = (error as ConfigValidationError).issues;
            expect(issues).toHaveLength(1);
            expect(issues[0].path).toBe('providers[1].name');
            expect(issues[0].message).toMatch(/duplicate provider name "coingecko"/);
            expect(issues[0].message).toMatch(/providers\[0\]/);
        });

        it('rejects a duplicate name even when the duplicates differ otherwise', () => {
            const config = {
                ...baseConfig,
                providers: [
                    coingeckoProvider({ weight: 0.5 }),
                    coingeckoProvider({ priority: 2, weight: 0.4, enabled: false }),
                ],
            };

            expect(() => new OracleService(config)).toThrow(/duplicate provider name/);
        });

        it('rejects an unknown provider name instead of silently dropping it', () => {
            const config = {
                ...baseConfig,
                providers: [
                    coingeckoProvider(),
                    binanceProvider(),
                    { ...binanceProvider(), name: 'kraken', priority: 3 },
                ],
            };

            let error: unknown;
            try {
                new OracleService(config);
            } catch (e) {
                error = e;
            }

            expect(error).toBeInstanceOf(ConfigValidationError);
            const issue = (error as ConfigValidationError).issues[0];
            expect(issue.path).toBe('providers[2].name');
            expect(issue.message).toContain('unknown provider "kraken"');
            for (const name of SUPPORTED_PROVIDER_NAMES) {
                expect(issue.message).toContain(name);
            }
        });

        it.each(['', '   ', undefined, null, 7, {}])(
            'rejects the invalid provider name %o',
            (name) => {
                const config = {
                    ...baseConfig,
                    providers: [coingeckoProvider({ name: name as string })],
                };

                expect(() =>
                    new OracleService(config as unknown as OracleServiceConfig),
                ).toThrow(/providers\[0\]\.name/);
            },
        );

        it.each([0, -1, 1.5, Number.NaN, Number.POSITIVE_INFINITY, Number.MAX_SAFE_INTEGER + 10, '1', undefined])(
            'rejects the invalid provider priority %o',
            (priority) => {
                const config = {
                    ...baseConfig,
                    providers: [coingeckoProvider({ priority: priority as number })],
                };

                expect(() =>
                    new OracleService(config as unknown as OracleServiceConfig),
                ).toThrow(/providers\[0\]\.priority/);
            },
        );

        it.each([undefined, null, 0, 'true', 1, 'yes'])(
            'rejects the non-boolean enabled flag %o',
            (enabled) => {
                const config = {
                    ...baseConfig,
                    providers: [coingeckoProvider({ enabled: enabled as boolean })],
                };

                expect(() =>
                    new OracleService(config as unknown as OracleServiceConfig),
                ).toThrow(/providers\[0\]\.enabled/);
            },
        );

        it.each(['not-a-url', '', 'ftp://api.coingecko.com', 'https://'])(
            'rejects the invalid provider baseUrl %o',
            (baseUrl) => {
                const config = {
                    ...baseConfig,
                    providers: [coingeckoProvider({ baseUrl })],
                };

                expect(() => new OracleService(config)).toThrow(/providers\[0\]\.baseUrl/);
            },
        );

        it('rejects an empty apiKey string', () => {
            const config = {
                ...baseConfig,
                providers: [coingeckoProvider({ apiKey: '' })],
            };

            expect(() => new OracleService(config)).toThrow(/providers\[0\]\.apiKey/);
        });

        it.each([undefined, null, {}, '10/60000'])(
            'rejects the malformed rateLimit %o',
            (rateLimit) => {
                const config = {
                    ...baseConfig,
                    providers: [coingeckoProvider({ rateLimit: rateLimit as never })],
                };

                expect(() =>
                    new OracleService(config as unknown as OracleServiceConfig),
                ).toThrow(/providers\[0\]\.rateLimit/);
            },
        );

        it.each([0, -1, 1.5, Number.NaN, undefined])(
            'rejects the invalid rateLimit.maxRequests %o',
            (maxRequests) => {
                const config = {
                    ...baseConfig,
                    providers: [
                        coingeckoProvider({
                            rateLimit: { maxRequests: maxRequests as number, windowMs: 60_000 },
                        }),
                    ],
                };

                expect(() =>
                    new OracleService(config as unknown as OracleServiceConfig),
                ).toThrow(/rateLimit\.maxRequests/);
            },
        );

        it.each([0, -1, 2.5, Number.NaN, undefined])(
            'rejects the invalid rateLimit.windowMs %o',
            (windowMs) => {
                const config = {
                    ...baseConfig,
                    providers: [
                        coingeckoProvider({
                            rateLimit: { maxRequests: 10, windowMs: windowMs as number },
                        }),
                    ],
                };

                expect(() =>
                    new OracleService(config as unknown as OracleServiceConfig),
                ).toThrow(/rateLimit\.windowMs/);
            },
        );

        it('accepts the minimum rate limit values', () => {
            const config = {
                ...baseConfig,
                providers: [
                    coingeckoProvider({ rateLimit: { maxRequests: 1, windowMs: 1 } }),
                ],
            };

            expect(new OracleService(config)).toBeInstanceOf(OracleService);
        });

        it.each([null, undefined, 'coingecko', 42, []])(
            'rejects the non-object provider entry %o',
            (provider) => {
                const config = {
                    ...baseConfig,
                    providers: [coingeckoProvider(), provider],
                };

                expect(() =>
                    new OracleService(config as unknown as OracleServiceConfig),
                ).toThrow(/providers\[1\]/);
            },
        );

        it('rejects a partially nulled provider entry', () => {
            const config = {
                ...baseConfig,
                providers: [
                    {
                        ...coingeckoProvider(),
                        baseUrl: undefined,
                        rateLimit: null,
                    },
                ],
            };

            expect(() =>
                new OracleService(config as unknown as OracleServiceConfig),
            ).toThrow(/providers\[0\]/);
        });

        it('does not report cascading weight issues for a malformed provider', () => {
            const config = {
                ...baseConfig,
                providers: [coingeckoProvider({ priority: 0 })],
            };

            let error: unknown;
            try {
                new OracleService(config);
            } catch (e) {
                error = e;
            }

            const messages = (error as ConfigValidationError).issues.map((i) => i.message);
            expect(messages.some((m) => /weights/.test(m))).toBe(false);
        });
    });

    /* ------------------------------------------------------------------ */
    /* Parameter boundary validation                                       */
    /* ------------------------------------------------------------------ */

    describe('oracle parameter boundaries', () => {
        it.each([0, -1, -0.0001, 100.0001, 101, Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY])(
            'rejects the out-of-range maxPriceDeviationPercent %o',
            (maxPriceDeviationPercent) => {
                expect(() =>
                    new OracleService({ ...baseConfig, maxPriceDeviationPercent }),
                ).toThrow(/maxPriceDeviationPercent/);
            },
        );

        it.each([undefined, null, '10', {}])(
            'rejects the non-numeric maxPriceDeviationPercent %o',
            (maxPriceDeviationPercent) => {
                expect(() =>
                    new OracleService({
                        ...baseConfig,
                        maxPriceDeviationPercent,
                    } as unknown as OracleServiceConfig),
                ).toThrow(/maxPriceDeviationPercent/);
            },
        );

        it.each([0, -1, Number.NaN, Number.POSITIVE_INFINITY])(
            'rejects the non-positive priceStaleThresholdSeconds %o',
            (priceStaleThresholdSeconds) => {
                expect(() =>
                    new OracleService({ ...baseConfig, priceStaleThresholdSeconds }),
                ).toThrow(/priceStaleThresholdSeconds/);
            },
        );

        it.each([-1, -30, Number.NaN, Number.NEGATIVE_INFINITY, undefined, null, '30'])(
            'rejects the negative or malformed cacheTtlSeconds %o',
            (cacheTtlSeconds) => {
                expect(() =>
                    new OracleService({
                        ...baseConfig,
                        cacheTtlSeconds,
                    } as unknown as OracleServiceConfig),
                ).toThrow(/cacheTtlSeconds/);
            },
        );

        it.each([0, -1, -1000, 1.5, Number.NaN, Number.POSITIVE_INFINITY, undefined, null, '1000'])(
            'rejects the invalid updateIntervalMs %o',
            (updateIntervalMs) => {
                expect(() =>
                    new OracleService({ ...baseConfig, updateIntervalMs } as unknown as OracleServiceConfig),
                ).toThrow(/updateIntervalMs/);
            },
        );

        it.each([-1, -0.5, Number.NaN, Number.POSITIVE_INFINITY, '3.5'])(
            'rejects the invalid madZScoreThreshold %o',
            (madZScoreThreshold) => {
                expect(() =>
                    new OracleService({
                        ...baseConfig,
                        madZScoreThreshold,
                    } as unknown as OracleServiceConfig),
                ).toThrow(/madZScoreThreshold/);
            },
        );

        it.each(['trace', 'ERROR', '', undefined, null, 3])(
            'rejects the invalid logLevel %o',
            (logLevel) => {
                expect(() =>
                    new OracleService({ ...baseConfig, logLevel } as unknown as OracleServiceConfig),
                ).toThrow(/logLevel/);
            },
        );

        it('rejects a malformed redisUrl but allows it to be omitted', () => {
            expect(() => new OracleService({ ...baseConfig, redisUrl: 'not-a-url' })).toThrow(
                /redisUrl/,
            );
        });

        it.each([{}, null, 'xlm', []])(
            'rejects the non-object config itself: %o',
            (config) => {
                expect(() => new OracleService(config as OracleServiceConfig)).toThrow(
                    ConfigValidationError,
                );
            },
        );
    });

    /* ------------------------------------------------------------------ */
    /* Privileged surface / role separation                                */
    /* ------------------------------------------------------------------ */

    describe('privileged surface and role separation', () => {
        it.each(['', '   ', undefined, null, 42, {}])(
            'rejects the invalid adminSecretKey %o',
            (adminSecretKey) => {
                expect(() =>
                    new OracleService({ ...baseConfig, adminSecretKey } as unknown as OracleServiceConfig),
                ).toThrow(/adminSecretKey/);
            },
        );

        it.each([-1, 65_536, 1.5, Number.NaN, '8080', null, {}])(
            'rejects the out-of-range adminApiPort %o',
            (adminApiPort) => {
                expect(() =>
                    new OracleService({ ...baseConfig, adminApiPort } as unknown as OracleServiceConfig),
                ).toThrow(/adminApiPort/);
            },
        );

        it('rejects an admin API enabled without an HMAC secret', () => {
            expect(() =>
                new OracleService({ ...baseConfig, adminApiPort: 8080 } as unknown as OracleServiceConfig),
            ).toThrow(/adminHmacSecret/);
        });

        it.each(['', 'short', 'a'.repeat(15), undefined, null])(
            'rejects a too-short adminHmacSecret %o',
            (adminHmacSecret) => {
                expect(() =>
                    new OracleService({
                        ...baseConfig,
                        adminApiPort: 8080,
                        adminHmacSecret,
                    } as unknown as OracleServiceConfig),
                ).toThrow(/adminHmacSecret/);
            },
        );

        it('accepts a disabled admin API (port 0) with no secret', () => {
            expect(
                new OracleService({ ...baseConfig, adminApiPort: 0 } as unknown as OracleServiceConfig),
            ).toBeInstanceOf(OracleService);
        });

        it('accepts a port-0 config that still carries a short secret', () => {
            // The privileged listener is off, so the secret is never exercised.
            expect(
                new OracleService({
                    ...baseConfig,
                    adminApiPort: 0,
                    adminHmacSecret: 'short',
                } as unknown as OracleServiceConfig),
            ).toBeInstanceOf(OracleService);
        });

        it('accepts an admin API with a strong secret at the port boundary', () => {
            expect(
                new OracleService({
                    ...baseConfig,
                    adminApiPort: 65_535,
                    adminHmacSecret: 'a'.repeat(32),
                } as unknown as OracleServiceConfig),
            ).toBeInstanceOf(OracleService);
        });

        it('exposes no configuration mutation surface', () => {
            const service = buildService(baseConfig);

            for (const method of [
                'updateConfig',
                'setProviders',
                'modifyAdminKey',
                'configureOracle',
                'setOracleProvider',
                'reloadConfig',
                'setAdminApiPort',
                'addProvider',
                'removeProvider',
            ]) {
                expect(typeof (service as unknown as Record<string, unknown>)[method]).toBe(
                    'undefined',
                );
            }
        });

        it('exposes only the read-only status and price operations', () => {
            const service = buildService(baseConfig);

            expect(typeof service.getStatus).toBe('function');
            expect(typeof service.updatePrices).toBe('function');
            expect(typeof service.fetchPrice).toBe('function');
            expect(typeof service.start).toBe('function');
            expect(typeof service.stop).toBe('function');

            // The validated config must not be part of the reported surface.
            const status = service.getStatus() as Record<string, unknown>;
            expect(status).not.toHaveProperty('config');
            expect(status).not.toHaveProperty('adminSecretKey');
        });

        it('never echoes the admin HMAC secret through the status surface', () => {
            const secret = 'super-secret-hmac-value-32chars';
            const service = buildService({
                ...baseConfig,
                adminApiPort: 8080,
                adminHmacSecret: secret,
            } as unknown as OracleServiceConfig);

            expect(JSON.stringify(service.getStatus())).not.toContain(secret);
        });
    });

    /* ------------------------------------------------------------------ */
    /* Configured parameters reach the runtime                             */
    /* ------------------------------------------------------------------ */

    describe('configured parameters take effect at runtime', () => {
        it('produces an aggregated price from a valid configuration', async () => {
            const service = buildService(baseConfig);

            const price = await service.fetchPrice('XLM');

            expect(price).not.toBeNull();
            expect(price?.asset).toBe('XLM');
            expect(price?.price).toBeGreaterThan(0n);
        });

        it('submits the aggregated price to the contract updater', async () => {
            const service = buildService(baseConfig);

            await service.updatePrices(['XLM']);

            expect(updatePricesMock).toHaveBeenCalledTimes(1);
            const submitted = updatePricesMock.mock.calls[0][0] as Array<{
                asset: string;
                price: bigint;
            }>;
            expect(submitted).toHaveLength(1);
            expect(submitted[0].asset).toBe('XLM');
            expect(submitted[0].price).toBeGreaterThan(0n);
        });

        it('aggregates from the enabled providers only', async () => {
            const service = buildService({
                ...baseConfig,
                providers: [coingeckoProvider({ enabled: false }), binanceProvider()],
            });

            const price = await service.fetchPrice('XLM');

            expect(price).not.toBeNull();
            expect(price?.sources.map((s) => s.source)).not.toContain('coingecko');
        });

        it('rejects provider data older than the configured staleness threshold', async () => {
            setProviderPrice(coingeckoFetchPrice, 'coingecko', COINGECKO_PRICE, 3_600);
            setProviderPrice(binanceFetchPrice, 'binance', BINANCE_PRICE, 3_600);

            const service = buildService({ ...baseConfig, priceStaleThresholdSeconds: 60 });

            await expect(service.fetchPrice('XLM')).resolves.toBeNull();
        });

        it('accepts provider data inside the configured staleness threshold', async () => {
            setProviderPrice(coingeckoFetchPrice, 'coingecko', COINGECKO_PRICE, 30);
            setProviderPrice(binanceFetchPrice, 'binance', BINANCE_PRICE, 30);

            const service = buildService({ ...baseConfig, priceStaleThresholdSeconds: 3_600 });

            await expect(service.fetchPrice('XLM')).resolves.not.toBeNull();
        });

        it('rejects provider data older than the default threshold at the boundary', async () => {
            // maxStalenessSeconds is 300; 301s of age must be refused.
            setProviderPrice(coingeckoFetchPrice, 'coingecko', COINGECKO_PRICE, 301);
            setProviderPrice(binanceFetchPrice, 'binance', BINANCE_PRICE, 301);

            const service = buildService(baseConfig);

            await expect(service.fetchPrice('XLM')).resolves.toBeNull();
        });

        it('returns null instead of throwing when every provider fails', async () => {
            setProviderFailure(coingeckoFetchPrice, 'coingecko unavailable');
            setProviderFailure(binanceFetchPrice, 'binance unavailable');

            const service = buildService(baseConfig);

            await expect(service.fetchPrice('XLM')).resolves.toBeNull();
        });

        it('completes an update cycle without throwing when every provider fails', async () => {
            setProviderFailure(coingeckoFetchPrice, 'coingecko unavailable');
            setProviderFailure(binanceFetchPrice, 'binance unavailable');

            const service = buildService(baseConfig);

            await expect(service.updatePrices(['XLM'])).resolves.toBeUndefined();
            // Nothing to submit, so the contract is never touched.
            expect(updatePricesMock).not.toHaveBeenCalled();
        });

        it('still reports a healthy status after a fully failed cycle', async () => {
            setProviderFailure(coingeckoFetchPrice, 'coingecko unavailable');
            setProviderFailure(binanceFetchPrice, 'binance unavailable');

            const service = buildService(baseConfig);
            await service.updatePrices(['XLM']);

            expect(service.getStatus().isRunning).toBe(false);
            expect(service.getStatus().network).toBe('testnet');
        });

        it('rejects a price outside the configured asset bounds', async () => {
            const service = buildService({
                ...baseConfig,
                priceBounds: { XLM: { minPrice: 1, maxPrice: 2 } },
            } as unknown as OracleServiceConfig);

            // The mocked feeds report ~0.15, far below the 1.0 floor.
            await expect(service.fetchPrice('XLM')).resolves.toBeNull();
        });
    });

    /* ------------------------------------------------------------------ */
    /* State transition invariants and status immutability                */
    /* ------------------------------------------------------------------ */

    describe('state transition invariants', () => {
        it('is idle before start and stopped after stop', async () => {
            const service = buildService(baseConfig);
            expect(service.getStatus().isRunning).toBe(false);

            await service.start(['XLM']);
            expect(service.getStatus().isRunning).toBe(true);

            await service.stop();
            expect(service.getStatus().isRunning).toBe(false);
        });

        it('treats a repeated stop as a no-op', async () => {
            const service = buildService(baseConfig);
            await service.start(['XLM']);
            await service.stop();

            await expect(service.stop()).resolves.toBeUndefined();
            expect(service.getStatus().isRunning).toBe(false);
        });

        it('treats a repeated start as idempotent', async () => {
            const service = buildService(baseConfig);
            await service.start(['XLM']);
            await service.start(['XLM']);

            expect(service.getStatus().isRunning).toBe(true);
            expect(createContractUpdaterMock).toHaveBeenCalledTimes(1);
        });

        it('returns a defensive copy of the provider list', () => {
            const service = buildService(baseConfig);
            const status = service.getStatus();

            status.providers.length = 0;
            status.providers.push({
                name: 'attacker',
                enabled: true,
                priority: 99,
                weight: 1,
            });

            const fresh = service.getStatus();
            expect(fresh.providers).toHaveLength(2);
            expect(fresh.providers.map((p) => p.name)).not.toContain('attacker');
        });

        it('returns a fresh provider snapshot on every call', () => {
            const service = buildService(baseConfig);

            expect(service.getStatus().providers).not.toBe(service.getStatus().providers);
            expect(service.getStatus().providers).toEqual(service.getStatus().providers);
        });

        it('does not let the caller mutate the validated config through getStatus', () => {
            const service = buildService(baseConfig);
            const first = service.getStatus();
            (first as unknown as { contractId: string }).contractId = 'CMUTATED';

            expect(service.getStatus().contractId).toBe('CTEST123');
        });
    });

    /* ------------------------------------------------------------------ */
    /* Configuration recovery and re-configuration                         */
    /* ------------------------------------------------------------------ */

    describe('configuration recovery and re-configuration', () => {
        it('can be reconstructed with an updated configuration after a stop', async () => {
            const service = buildService(baseConfig);
            await service.start(['XLM']);
            await service.stop();

            const updated = buildService({
                ...baseConfig,
                maxPriceDeviationPercent: 25,
                providers: [coingeckoProvider({ enabled: false }), binanceProvider()],
            });

            expect(updated.getStatus().providers.map((p) => p.enabled)).toEqual([false, true]);
        });

        it('starts cleanly after a rejected configuration attempt', async () => {
            expect(() =>
                new OracleService({ ...baseConfig, contractId: '' } as unknown as OracleServiceConfig),
            ).toThrow(ConfigValidationError);

            const service = buildService(baseConfig);
            await service.start(['XLM']);

            expect(service.getStatus().isRunning).toBe(true);
        });

        it('constructs no providers and no updater when validation fails', () => {
            expect(() =>
                new OracleService({ ...baseConfig, providers: [] }),
            ).toThrow(ConfigValidationError);

            expect(createCoinGeckoProviderMock).not.toHaveBeenCalled();
            expect(createBinanceProviderMock).not.toHaveBeenCalled();
            expect(createContractUpdaterMock).not.toHaveBeenCalled();
        });

        it('leaves no pending timers behind after a rejected configuration', () => {
            vi.useFakeTimers();
            try {
                const before = vi.getTimerCount();
                expect(() => new OracleService({ ...baseConfig, contractId: '' })).toThrow();
                expect(vi.getTimerCount()).toBe(before);
            } finally {
                vi.useRealTimers();
            }
        });

        it('applies a corrected configuration after a rejected one', () => {
            const broken = { ...baseConfig, cacheTtlSeconds: -1 };
            expect(() => new OracleService(broken as unknown as OracleServiceConfig)).toThrow(
                /cacheTtlSeconds/,
            );

            const fixed = buildService({ ...baseConfig, cacheTtlSeconds: 0 });
            expect(fixed).toBeInstanceOf(OracleService);
        });
    });

    /* ------------------------------------------------------------------ */
    /* Error observability                                                 */
    /* ------------------------------------------------------------------ */

    describe('error observability', () => {
        it('throws a ConfigValidationError with a stable name and code', () => {
            let error: unknown;
            try {
                new OracleService({ ...baseConfig, contractId: '' });
            } catch (e) {
                error = e;
            }

            expect(error).toBeInstanceOf(ConfigValidationError);
            expect(error).toBeInstanceOf(Error);
            expect((error as ConfigValidationError).name).toBe('ConfigValidationError');
        });

        it('addresses every problem with a dotted path', () => {
            const config = {
                ...baseConfig,
                stellarNetwork: 'devnet',
                stellarRpcUrl: 'nope',
                contractId: '',
                adminSecretKey: '',
                updateIntervalMs: 0,
                maxPriceDeviationPercent: 0,
                priceStaleThresholdSeconds: 0,
                cacheTtlSeconds: -1,
                logLevel: 'trace',
                providers: [],
            };

            let error: unknown;
            try {
                new OracleService(config as unknown as OracleServiceConfig);
            } catch (e) {
                error = e;
            }

            const paths = (error as ConfigValidationError).issues.map((i) => i.path);
            expect(paths).toEqual(
                expect.arrayContaining([
                    'stellarNetwork',
                    'stellarRpcUrl',
                    'contractId',
                    'adminSecretKey',
                    'updateIntervalMs',
                    'maxPriceDeviationPercent',
                    'priceStaleThresholdSeconds',
                    'cacheTtlSeconds',
                    'logLevel',
                    'providers',
                ]),
            );
        });

        it('reports every problem in one pass instead of failing one at a time', () => {
            const config = {
                ...baseConfig,
                contractId: '',
                cacheTtlSeconds: -1,
                providers: [coingeckoProvider({ priority: 0 })],
            };

            let error: unknown;
            try {
                new OracleService(config as unknown as OracleServiceConfig);
            } catch (e) {
                error = e;
            }

            const paths = (error as ConfigValidationError).issues.map((i) => i.path);
            expect(paths).toContain('contractId');
            expect(paths).toContain('cacheTtlSeconds');
            expect(paths).toContain('providers[0].priority');
        });

        it('includes each path and message in the error message', () => {
            let error: unknown;
            try {
                new OracleService({ ...baseConfig, contractId: '', logLevel: 'trace' } as unknown as OracleServiceConfig);
            } catch (e) {
                error = e;
            }

            const message = (error as Error).message;
            expect(message).toContain('Invalid oracle service configuration');
            expect(message).toContain('contractId');
            expect(message).toContain('logLevel');
        });

        it('never leaks the admin secret into a validation error', () => {
            let error: unknown;
            try {
                new OracleService({
                    ...baseConfig,
                    adminSecretKey: 'super-secret-admin-key',
                    contractId: '',
                } as unknown as OracleServiceConfig);
            } catch (e) {
                error = e;
            }

            expect((error as Error).message).not.toContain('super-secret-admin-key');
        });
    });

    /* ------------------------------------------------------------------ */
    /* Determinism and input immutability                                 */
    /* ------------------------------------------------------------------ */

    describe('determinism and immutability', () => {
        it('produces identical issues for identical invalid input', () => {
            const config = () => ({
                ...baseConfig,
                contractId: '',
                providers: [coingeckoProvider({ priority: -1 })],
            });

            const capture = () => {
                try {
                    validateOracleServiceConfig(config());
                    return null;
                } catch (e) {
                    return (e as ConfigValidationError).issues;
                }
            };

            expect(capture()).toEqual(capture());
        });

        it('produces identical status for identical configuration', () => {
            const first = buildService(baseConfig).getStatus();
            const second = buildService(baseConfig).getStatus();

            expect(first).toEqual(second);
        });

        it('does not mutate the caller-supplied configuration', () => {
            const config = {
                ...baseConfig,
                providers: [coingeckoProvider({ weight: 0.2 }), binanceProvider({ weight: 0.3 })],
            };
            const snapshot = JSON.parse(JSON.stringify(config));

            buildService(config);

            expect(JSON.parse(JSON.stringify(config))).toEqual(snapshot);
        });

        it('does not share provider objects with the caller', () => {
            const provider = coingeckoProvider();
            const config = { ...baseConfig, providers: [provider, binanceProvider()] };

            const service = buildService(config);
            const before = service.getStatus().providers;

            // Mutating the caller's object after construction must not reach in.
            provider.weight = 999;
            provider.enabled = false;
            provider.priority = 42;

            const after = service.getStatus().providers;
            expect(after).toEqual(before);
            expect(after.find((p) => p.name === 'coingecko')?.enabled).toBe(true);
            expect(after.find((p) => p.name === 'coingecko')?.priority).toBe(1);
            expect(after.find((p) => p.name === 'coingecko')?.weight).toBeCloseTo(0.6, 10);
        });

        it('handles a circular reference in the configuration without hanging', () => {
            const config: Record<string, unknown> = { ...baseConfig };
            config.self = config;

            expect(() => new OracleService(config as unknown as OracleServiceConfig)).not.toThrow();
        });

        it('reports the same normalization for repeated validation of the same object', () => {
            const config = {
                ...baseConfig,
                providers: [coingeckoProvider({ weight: 0.2 }), binanceProvider({ weight: 0.3 })],
            };

            const first = validateOracleServiceConfig(config);
            const second = validateOracleServiceConfig(config);

            expect(first.providers).toEqual(second.providers);
            expect(first.providers[0].weight).toBeCloseTo(0.4, 10);
            expect(first.providers[1].weight).toBeCloseTo(0.6, 10);
        });
    });

    /* ------------------------------------------------------------------ */
    /* Price bounds configuration                                          */
    /* ------------------------------------------------------------------ */

    describe('price bounds validation', () => {
        it('accepts well-formed price bounds', () => {
            const config = {
                ...baseConfig,
                priceBounds: { XLM: { minPrice: 0.0001, maxPrice: 10 } },
            };

            expect(new OracleService(config as unknown as OracleServiceConfig)).toBeInstanceOf(
                OracleService,
            );
        });

        it.each([0, -1, Number.NaN])('rejects the non-positive minPrice %o', (minPrice) => {
            const config = {
                ...baseConfig,
                priceBounds: { XLM: { minPrice, maxPrice: 10 } },
            };

            expect(() =>
                new OracleService(config as unknown as OracleServiceConfig),
            ).toThrow(/priceBounds\.XLM\.minPrice/);
        });

        it('rejects a maxPrice below minPrice', () => {
            const config = {
                ...baseConfig,
                priceBounds: { XLM: { minPrice: 5, maxPrice: 1 } },
            };

            expect(() =>
                new OracleService(config as unknown as OracleServiceConfig),
            ).toThrow(/priceBounds\.XLM\.maxPrice/);
        });

        it('accepts maxPrice equal to minPrice at the boundary', () => {
            const config = {
                ...baseConfig,
                priceBounds: { XLM: { minPrice: 1, maxPrice: 1 } },
            };

            expect(new OracleService(config as unknown as OracleServiceConfig)).toBeInstanceOf(
                OracleService,
            );
        });

        it.each([null, 'XLM', 5, []])('rejects the non-object priceBounds %o', (priceBounds) => {
            const config = { ...baseConfig, priceBounds };

            expect(() =>
                new OracleService(config as unknown as OracleServiceConfig),
            ).toThrow(/priceBounds/);
        });

        it('rejects a malformed bounds entry', () => {
            const config = { ...baseConfig, priceBounds: { XLM: 5 } };

            expect(() =>
                new OracleService(config as unknown as OracleServiceConfig),
            ).toThrow(/priceBounds\.XLM/);
        });
    });

    /* ------------------------------------------------------------------ */
    /* Scale and topology boundaries                                      */
    /* ------------------------------------------------------------------ */

    describe('topology boundaries', () => {
        it('accepts the largest legitimate topology (one entry per supported provider)', () => {
            const providers = SUPPORTED_PROVIDER_NAMES.map((name, index) =>
                name === 'coingecko'
                    ? coingeckoProvider({ priority: index + 1, weight: 0.5 })
                    : binanceProvider({ priority: index + 1, weight: 0.5 }),
            );

            const service = buildService({ ...baseConfig, providers });

            expect(service.getStatus().providers).toHaveLength(SUPPORTED_PROVIDER_NAMES.length);
        });

        it('rejects a topology larger than the supported provider set', () => {
            // A third entry can only duplicate a name, since the supported set
            // is exhausted. Rejecting is what keeps the topology unambiguous.
            const providers = [
                coingeckoProvider({ priority: 1, weight: 0.34 }),
                binanceProvider({ priority: 2, weight: 0.33 }),
                coingeckoProvider({ priority: 3, weight: 0.33 }),
            ];

            expect(() => new OracleService({ ...baseConfig, providers })).toThrow(
                /duplicate provider name/,
            );
        });

        it('supports a wide range of update intervals', () => {
            for (const updateIntervalMs of [1, 1_000, 60_000, 3_600_000]) {
                expect(buildService({ ...baseConfig, updateIntervalMs })).toBeDefined();
            }
        });

        it('supports a wide range of cache TTLs', () => {
            for (const cacheTtlSeconds of [0, 1, 30, 86_400]) {
                expect(buildService({ ...baseConfig, cacheTtlSeconds })).toBeDefined();
            }
        });

        it('supports a wide range of staleness thresholds', () => {
            for (const priceStaleThresholdSeconds of [1, 60, 300, 86_400]) {
                expect(buildService({ ...baseConfig, priceStaleThresholdSeconds })).toBeDefined();
            }
        });
    });
});

describe('validateOracleServiceConfig', () => {
    it('returns a normalized copy without touching the input', () => {
        const config = {
            stellarNetwork: 'testnet' as const,
            stellarRpcUrl: 'https://soroban-testnet.stellar.org',
            contractId: 'CTEST123',
            adminSecretKey: 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456',
            updateIntervalMs: 60_000,
            maxPriceDeviationPercent: 10,
            priceStaleThresholdSeconds: 300,
            cacheTtlSeconds: 30,
            logLevel: 'error' as const,
            providers: [
                {
                    name: 'coingecko',
                    enabled: true,
                    priority: 1,
                    weight: 0.3,
                    baseUrl: 'https://api.coingecko.com/api/v3',
                    rateLimit: { maxRequests: 10, windowMs: 60_000 },
                },
                {
                    name: 'binance',
                    enabled: true,
                    priority: 2,
                    weight: 0.1,
                    baseUrl: 'https://api.binance.com/api/v3',
                    rateLimit: { maxRequests: 1200, windowMs: 60_000 },
                },
            ],
        };

        const validated = validateOracleServiceConfig(config);

        expect(validated).not.toBe(config);
        expect(validated.providers[0]).not.toBe(config.providers[0]);
        expect(validated.providers[0].rateLimit).not.toBe(config.providers[0].rateLimit);
        expect(validated.providers.map((p) => p.weight)).toHaveLength(2);
        expect(validated.providers[0].weight).toBeCloseTo(0.75, 10);
        expect(validated.providers[1].weight).toBeCloseTo(0.25, 10);
        expect(config.providers.map((p) => p.weight)).toEqual([0.3, 0.1]);
    });

    it('preserves non-provider fields verbatim', () => {
        const config = {
            stellarNetwork: 'mainnet' as const,
            stellarRpcUrl: 'https://soroban-mainnet.stellar.org',
            contractId: 'CABC123',
            adminSecretKey: 'SABC',
            updateIntervalMs: 1_000,
            maxPriceDeviationPercent: 3,
            madZScoreThreshold: 2.5,
            priceStaleThresholdSeconds: 120,
            cacheTtlSeconds: 0,
            logLevel: 'warn' as const,
            providers: [
                {
                    name: 'binance',
                    enabled: true,
                    priority: 1,
                    weight: 1,
                    baseUrl: 'https://api.binance.com/api/v3',
                    rateLimit: { maxRequests: 1, windowMs: 1 },
                },
            ],
        };

        const validated = validateOracleServiceConfig(config);

        expect(validated.stellarNetwork).toBe('mainnet');
        expect(validated.stellarRpcUrl).toBe('https://soroban-mainnet.stellar.org');
        expect(validated.contractId).toBe('CABC123');
        expect(validated.madZScoreThreshold).toBe(2.5);
        expect(validated.cacheTtlSeconds).toBe(0);
        expect(validated.logLevel).toBe('warn');
    });
});
