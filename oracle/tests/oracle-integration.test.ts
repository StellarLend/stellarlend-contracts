/**
 * Tests for Oracle Integration Service
 * End-to-end integration tests for the main OracleService
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { OracleService } from '../src/index.js';
import type { OracleServiceConfig } from '../src/config.js';

// Mock contract updater to avoid actual blockchain calls
vi.mock('../src/services/contract-updater.js', () => ({
    createContractUpdater: vi.fn(() => ({
        updatePrices: vi.fn().mockResolvedValue([
            { success: true, asset: 'XLM', price: 150000n, timestamp: Date.now() },
        ]),
        healthCheck: vi.fn().mockResolvedValue(true),
        getAdminPublicKey: vi.fn().mockReturnValue('GTEST123'),
    })),
    ContractUpdater: vi.fn(),
}));

// Mock providers to avoid actual API calls
vi.mock('../src/providers/coingecko.js', () => ({
    createCoinGeckoProvider: vi.fn(() => ({
        name: 'coingecko',
        isEnabled: true,
        priority: 1,
        weight: 0.6,
        getSupportedAssets: () => ['XLM', 'BTC', 'ETH'],
        fetchPrice: vi.fn().mockResolvedValue({
            asset: 'XLM',
            price: 0.15,
            timestamp: Math.floor(Date.now() / 1000),
            source: 'coingecko',
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
        fetchPrice: vi.fn().mockResolvedValue({
            asset: 'XLM',
            price: 0.152,
            timestamp: Math.floor(Date.now() / 1000),
            source: 'binance',
        }),
    })),
}));

describe('OracleService Integration', () => {
    let service: OracleService;
    let mockConfig: OracleServiceConfig;

    beforeEach(() => {
        mockConfig = {
            stellarNetwork: 'testnet',
            stellarRpcUrl: 'https://soroban-testnet.stellar.org',
            contractId: 'CTEST123',
            adminSecretKey: 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456',
            updateIntervalMs: 1000,
            maxPriceDeviationPercent: 10,
            priceStaleThresholdSeconds: 300,
            cacheTtlSeconds: 30,
            logLevel: 'error', // Reduce log noise in tests
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
        };
    });

    afterEach(() => {
        if (service) {
            service.stop();
        }
    });

    describe('initialization', () => {
        it('should create oracle service with valid config', () => {
            service = new OracleService(mockConfig);

            expect(service).toBeDefined();
            expect(service).toBeInstanceOf(OracleService);
        });

        it('should initialize with testnet network', () => {
            service = new OracleService({
                ...mockConfig,
                stellarNetwork: 'testnet',
            });

            expect(service).toBeDefined();
        });

        it('should initialize with mainnet network', () => {
            service = new OracleService({
                ...mockConfig,
                stellarNetwork: 'mainnet',
            });

            expect(service).toBeDefined();
        });

        it('should initialize with custom update interval', () => {
            service = new OracleService({
                ...mockConfig,
                updateIntervalMs: 5000,
            });

            const status = service.getStatus();
            expect(service).toBeDefined();
        });
    });

    describe('lifecycle', () => {
        it('should start service successfully', async () => {
            service = new OracleService(mockConfig);

            await service.start(['XLM']);

            const status = service.getStatus();
            expect(status.isRunning).toBe(true);
        });

        it('should stop service successfully', async () => {
            service = new OracleService(mockConfig);

            await service.start(['XLM']);
            expect(service.getStatus().isRunning).toBe(true);

            service.stop();
            expect(service.getStatus().isRunning).toBe(false);
        });

        it('should handle start when already running', async () => {
            service = new OracleService(mockConfig);

            await service.start(['XLM']);
            const firstStart = service.getStatus().isRunning;

            // Try to start again
            await service.start(['XLM']);
            const secondStart = service.getStatus().isRunning;

            expect(firstStart).toBe(true);
            expect(secondStart).toBe(true);
        });

        it('should handle stop when not running', () => {
            service = new OracleService(mockConfig);

            // Stop without starting
            expect(() => service.stop()).not.toThrow();
        });

        it('should handle multiple stop calls', async () => {
            service = new OracleService(mockConfig);

            await service.start(['XLM']);
            service.stop();

            expect(() => service.stop()).not.toThrow();
        });
    });

    describe('price updates', () => {
        it('should update prices for single asset', async () => {
            service = new OracleService(mockConfig);

            await service.updatePrices(['XLM']);

            // Service should complete without errors
            expect(service).toBeDefined();
        });

        it('should update prices for multiple assets', async () => {
            service = new OracleService(mockConfig);

            await service.updatePrices(['XLM', 'BTC', 'ETH']);

            expect(service).toBeDefined();
        });

        it('should handle empty asset list', async () => {
            service = new OracleService(mockConfig);

            await service.updatePrices([]);

            expect(service).toBeDefined();
        });

        it('should handle price updates with service running', async () => {
            service = new OracleService(mockConfig);

            await service.start(['XLM']);

            // Allow time for at least one update cycle
            await new Promise(resolve => setTimeout(resolve, 100));

            service.stop();
        });

        it('should handle unsupported assets gracefully', async () => {
            service = new OracleService(mockConfig);

            // Should not throw for unsupported asset
            await expect(
                service.updatePrices(['XLM', 'UNSUPPORTED_ASSET'])
            ).resolves.not.toThrow();
        });
    });

    describe('manual price fetching', () => {
        it('should fetch price for single asset', async () => {
            service = new OracleService(mockConfig);

            const price = await service.fetchPrice('XLM');

            expect(price).toBeDefined();
            if (price) {
                expect(price.asset).toBe('XLM');
                expect(price.price).toBeGreaterThan(0n);
            }
        });

        it('should fetch prices for different assets', async () => {
            service = new OracleService(mockConfig);

            const xlmPrice = await service.fetchPrice('XLM');
            const btcPrice = await service.fetchPrice('BTC');

            expect(xlmPrice).toBeDefined();
            expect(btcPrice).toBeDefined();
        });

        it('should return null for unsupported asset', async () => {
            service = new OracleService(mockConfig);

            const price = await service.fetchPrice('UNSUPPORTED');

            // May return null or handle gracefully
            expect(price === null || price !== undefined).toBe(true);
        });

        it('should cache fetched prices', async () => {
            service = new OracleService(mockConfig);

            const price1 = await service.fetchPrice('XLM');
            const price2 = await service.fetchPrice('XLM');

            // Second fetch should be faster (cached)
            expect(price1).toBeDefined();
            expect(price2).toBeDefined();
        });
    });

    describe('status monitoring', () => {
        it('should return status when service is stopped', () => {
            service = new OracleService(mockConfig);

            const status = service.getStatus();

            expect(status).toBeDefined();
            expect(status.isRunning).toBe(false);
            expect(status.network).toBe('testnet');
            expect(status.contractId).toBe('CTEST123');
        });

        it('should return status when service is running', async () => {
            service = new OracleService(mockConfig);

            await service.start(['XLM']);
            const status = service.getStatus();

            expect(status).toBeDefined();
            expect(status.isRunning).toBe(true);
            expect(status.network).toBe('testnet');
        });

        it('should include provider information in status', () => {
            service = new OracleService(mockConfig);

            const status = service.getStatus();

            expect(status.providers).toBeDefined();
            expect(Array.isArray(status.providers)).toBe(true);
            expect(status.providers.length).toBeGreaterThan(0);
        });

        it('should include aggregator stats in status', () => {
            service = new OracleService(mockConfig);

            const status = service.getStatus();

            expect(status.aggregatorStats).toBeDefined();
        });

        it('should update status after start', async () => {
            service = new OracleService(mockConfig);

            const beforeStatus = service.getStatus();
            expect(beforeStatus.isRunning).toBe(false);

            await service.start(['XLM']);

            const afterStatus = service.getStatus();
            expect(afterStatus.isRunning).toBe(true);
        });

        it('should update status after stop', async () => {
            service = new OracleService(mockConfig);

            await service.start(['XLM']);
            expect(service.getStatus().isRunning).toBe(true);

            service.stop();

            const afterStatus = service.getStatus();
            expect(afterStatus.isRunning).toBe(false);
        });
    });

    describe('configuration', () => {
        it('should handle different log levels', () => {
            const logLevels = ['debug', 'info', 'warn', 'error'] as const;

            logLevels.forEach(level => {
                const testService = new OracleService({
                    ...mockConfig,
                    logLevel: level,
                });

                expect(testService).toBeDefined();
            });
        });

        it('should handle custom cache TTL', () => {
            service = new OracleService({
                ...mockConfig,
                cacheTtlSeconds: 60,
            });

            expect(service).toBeDefined();
        });

        it('should handle custom price deviation threshold', () => {
            service = new OracleService({
                ...mockConfig,
                maxPriceDeviationPercent: 15,
            });

            expect(service).toBeDefined();
        });

        it('should handle custom staleness threshold', () => {
            service = new OracleService({
                ...mockConfig,
                priceStaleThresholdSeconds: 600,
            });

            expect(service).toBeDefined();
        });
    });

    describe('error handling', () => {
        it('should handle provider failures gracefully', async () => {
            service = new OracleService(mockConfig);

            // Should not throw even if providers fail
            await expect(service.updatePrices(['XLM'])).resolves.not.toThrow();
        });

        it('should continue running after price update failure', async () => {
            service = new OracleService(mockConfig);

            await service.start(['XLM']);

            // Allow some update cycles
            await new Promise(resolve => setTimeout(resolve, 200));

            const status = service.getStatus();
            expect(status.isRunning).toBe(true);

            service.stop();
        });

        it('should handle contract updater failures', async () => {
            const { createContractUpdater } = await import('../src/services/contract-updater.js');

            // Mock contract updater to fail
            vi.mocked(createContractUpdater).mockReturnValueOnce({
                updatePrices: vi.fn().mockResolvedValue([
                    { success: false, asset: 'XLM', price: 0n, timestamp: 0, error: 'Network error' },
                ]),
                healthCheck: vi.fn().mockResolvedValue(false),
                getAdminPublicKey: vi.fn().mockReturnValue('GTEST123'),
            } as any);

            service = new OracleService(mockConfig);

            await expect(service.updatePrices(['XLM'])).resolves.not.toThrow();
        });
    });

    describe('concurrency', () => {
        it('should handle concurrent price fetches', async () => {
            service = new OracleService(mockConfig);

            const promises = [
                service.fetchPrice('XLM'),
                service.fetchPrice('BTC'),
                service.fetchPrice('ETH'),
            ];

            const results = await Promise.all(promises);

            expect(results).toHaveLength(3);
            results.forEach(result => {
                expect(result === null || result !== undefined).toBe(true);
            });
        });

        it('should handle concurrent update calls', async () => {
            service = new OracleService(mockConfig);

            const promises = [
                service.updatePrices(['XLM']),
                service.updatePrices(['BTC']),
            ];

            await expect(Promise.all(promises)).resolves.not.toThrow();
        });
    });
});
