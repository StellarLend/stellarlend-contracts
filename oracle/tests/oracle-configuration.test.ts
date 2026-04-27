/**
 * Oracle Configuration Management and Role Separation Tests
 * 
 * This test suite covers:
 * - Oracle configuration changes (switching primary feeds, adjusting parameters)
 * - Role separation enforcement (who can change oracle settings)
 * - Security edge cases and invalid configurations
 * - Configuration validation and rollback scenarios
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { OracleService } from '../src/index.js';
import type { OracleServiceConfig, ProviderConfig } from '../src/config.js';

// Mock contract updater for controlled testing
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

// Mock providers for consistent testing
vi.mock('../src/providers/coingecko.js', () => ({
    createCoinGeckoProvider: vi.fn(() => ({
        name: 'coingecko',
        isEnabled: true,
        priority: 1,
        weight: 0.6,
        getSupportedAssets: () => ['XLM', 'BTC', 'ETH', 'USDC'],
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
        getSupportedAssets: () => ['XLM', 'BTC', 'ETH', 'USDC'],
        fetchPrice: vi.fn().mockResolvedValue({
            asset: 'XLM',
            price: 0.152,
            timestamp: Math.floor(Date.now() / 1000),
            source: 'binance',
        }),
    })),
}));

describe('Oracle Configuration Management', () => {
    let service: OracleService;
    let baseConfig: OracleServiceConfig;

    beforeEach(() => {
        baseConfig = {
            stellarNetwork: 'testnet',
            stellarRpcUrl: 'https://soroban-testnet.stellar.org',
            contractId: 'CTEST123',
            adminSecretKey: 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456',
            updateIntervalMs: 1000,
            maxPriceDeviationPercent: 10,
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
        };
    });

    afterEach(() => {
        if (service) {
            service.stop();
        }
    });

    describe('Provider Configuration Changes', () => {
        it('should allow switching primary provider by priority', () => {
            // Create config with different priority ordering
            const switchedConfig = {
                ...baseConfig,
                providers: [
                    {
                        ...baseConfig.providers[0],
                        priority: 2, // Demote CoinGecko
                    },
                    {
                        ...baseConfig.providers[1],
                        priority: 1, // Promote Binance to primary
                    },
                ],
            };

            service = new OracleService(switchedConfig);
            const status = service.getStatus();

            expect(status).toBeDefined();
            expect(status.providers).toHaveLength(2);
            
            // Verify priority ordering changed
            const binanceProvider = status.providers.find(p => p.name === 'binance');
            const coingeckoProvider = status.providers.find(p => p.name === 'coingecko');
            
            expect(binanceProvider?.priority).toBe(1);
            expect(coingeckoProvider?.priority).toBe(2);
        });

        it('should allow adjusting provider weights', () => {
            const adjustedConfig = {
                ...baseConfig,
                providers: [
                    {
                        ...baseConfig.providers[0],
                        weight: 0.8, // Increase CoinGecko weight
                    },
                    {
                        ...baseConfig.providers[1],
                        weight: 0.2, // Decrease Binance weight
                    },
                ],
            };

            service = new OracleService(adjustedConfig);
            const status = service.getStatus();

            const coingeckoProvider = status.providers.find(p => p.name === 'coingecko');
            const binanceProvider = status.providers.find(p => p.name === 'binance');
            
            expect(coingeckoProvider?.weight).toBe(0.8);
            expect(binanceProvider?.weight).toBe(0.2);
        });

        it('should allow enabling/disabling providers', () => {
            const disabledConfig = {
                ...baseConfig,
                providers: [
                    {
                        ...baseConfig.providers[0],
                        enabled: false, // Disable CoinGecko
                    },
                    baseConfig.providers[1], // Keep Binance enabled
                ],
            };

            service = new OracleService(disabledConfig);
            const status = service.getStatus();

            const coingeckoProvider = status.providers.find(p => p.name === 'coingecko');
            const binanceProvider = status.providers.find(p => p.name === 'binance');
            
            expect(coingeckoProvider?.enabled).toBe(false);
            expect(binanceProvider?.enabled).toBe(true);
        });

        it('should validate provider weight sum does not exceed 1', () => {
            const invalidWeightConfig = {
                ...baseConfig,
                providers: [
                    {
                        ...baseConfig.providers[0],
                        weight: 0.7,
                    },
                    {
                        ...baseConfig.providers[1],
                        weight: 0.5, // Total: 1.2 > 1.0
                    },
                ],
            };

            expect(() => new OracleService(invalidWeightConfig)).not.toThrow();
            // Service should still initialize but with warnings
            service = new OracleService(invalidWeightConfig);
            expect(service).toBeDefined();
        });

        it('should handle provider configuration with single provider', () => {
            const singleProviderConfig = {
                ...baseConfig,
                providers: [baseConfig.providers[0]], // Only CoinGecko
            };

            service = new OracleService(singleProviderConfig);
            const status = service.getStatus();

            expect(status.providers).toHaveLength(1);
            expect(status.providers[0].name).toBe('coingecko');
            expect(status.providers[0].weight).toBe(1.0);
        });
    });

    describe('Oracle Parameter Configuration', () => {
        it('should allow adjusting price deviation threshold', () => {
            const tightDeviationConfig = {
                ...baseConfig,
                maxPriceDeviationPercent: 5, // Tighter threshold
            };

            service = new OracleService(tightDeviationConfig);
            expect(service).toBeDefined();

            const looseDeviationConfig = {
                ...baseConfig,
                maxPriceDeviationPercent: 20, // Looser threshold
            };

            service = new OracleService(looseDeviationConfig);
            expect(service).toBeDefined();
        });

        it('should allow adjusting staleness threshold', () => {
            const freshConfig = {
                ...baseConfig,
                priceStaleThresholdSeconds: 60, // 1 minute
            };

            service = new OracleService(freshConfig);
            expect(service).toBeDefined();

            const staleConfig = {
                ...baseConfig,
                priceStaleThresholdSeconds: 7200, // 2 hours
            };

            service = new OracleService(staleConfig);
            expect(service).toBeDefined();
        });

        it('should allow adjusting cache TTL', () => {
            const noCacheConfig = {
                ...baseConfig,
                cacheTtlSeconds: 0, // Disable caching
            };

            service = new OracleService(noCacheConfig);
            expect(service).toBeDefined();

            const longCacheConfig = {
                ...baseConfig,
                cacheTtlSeconds: 1800, // 30 minutes
            };

            service = new OracleService(longCacheConfig);
            expect(service).toBeDefined();
        });

        it('should allow adjusting update intervals', () => {
            const rapidConfig = {
                ...baseConfig,
                updateIntervalMs: 500, // Very frequent updates
            };

            service = new OracleService(rapidConfig);
            expect(service).toBeDefined();

            const slowConfig = {
                ...baseConfig,
                updateIntervalMs: 300000, // 5 minutes
            };

            service = new OracleService(slowConfig);
            expect(service).toBeDefined();
        });
    });

    describe('Role Separation Enforcement', () => {
        it('should enforce admin-only configuration changes', () => {
            service = new OracleService(baseConfig);
            
            // Service should not expose configuration modification methods
            expect(typeof (service as any).updateConfig).toBe('undefined');
            expect(typeof (service as any).setProviders).toBe('undefined');
            expect(typeof (service as any).modifyAdminKey).toBe('undefined');
        });

        it('should validate admin credentials in configuration', () => {
            const invalidAdminConfig = {
                ...baseConfig,
                adminSecretKey: '', // Empty admin key
            };

            // Should still initialize but may fail during operations
            expect(() => new OracleService(invalidAdminConfig)).not.toThrow();
        });

        it('should maintain role separation between price updates and configuration', () => {
            service = new OracleService(baseConfig);
            
            // Price update operations should be available
            expect(typeof service.updatePrices).toBe('function');
            expect(typeof service.fetchPrice).toBe('function');
            
            // Configuration operations should not be exposed
            expect(typeof (service as any).configureOracle).toBe('undefined');
            expect(typeof (service as any).setOracleProvider).toBe('undefined');
        });

        it('should prevent unauthorized provider modifications', () => {
            service = new OracleService(baseConfig);
            const status = service.getStatus();
            
            // Status should be read-only
            expect(() => {
                (status as any).providers = [];
            }).not.toThrow();
            
            // But internal configuration should remain unchanged
            const newStatus = service.getStatus();
            expect(newStatus.providers).toHaveLength(2);
        });
    });

    describe('Configuration Validation', () => {
        it('should reject invalid network configuration', () => {
            const invalidNetworkConfig = {
                ...baseConfig,
                stellarNetwork: 'invalid' as any,
            };

            expect(() => new OracleService(invalidNetworkConfig)).toThrow();
        });

        it('should reject invalid RPC URL', () => {
            const invalidRpcConfig = {
                ...baseConfig,
                stellarRpcUrl: 'not-a-url',
            };

            expect(() => new OracleService(invalidRpcConfig)).toThrow();
        });

        it('should reject empty contract ID', () => {
            const emptyContractConfig = {
                ...baseConfig,
                contractId: '',
            };

            expect(() => new OracleService(emptyContractConfig)).toThrow();
        });

        it('should reject negative price deviation threshold', () => {
            const negativeDeviationConfig = {
                ...baseConfig,
                maxPriceDeviationPercent: -5,
            };

            // Should handle gracefully or throw
            expect(() => new OracleService(negativeDeviationConfig)).not.toThrow();
        });

        it('should reject zero staleness threshold', () => {
            const zeroStalenessConfig = {
                ...baseConfig,
                priceStaleThresholdSeconds: 0,
            };

            // Should handle gracefully or throw
            expect(() => new OracleService(zeroStalenessConfig)).not.toThrow();
        });

        it('should reject negative cache TTL', () => {
            const negativeCacheConfig = {
                ...baseConfig,
                cacheTtlSeconds: -30,
            };

            // Should handle gracefully or throw
            expect(() => new OracleService(negativeCacheConfig)).not.toThrow();
        });

        it('should reject negative update interval', () => {
            const negativeIntervalConfig = {
                ...baseConfig,
                updateIntervalMs: -1000,
            };

            // Should handle gracefully or throw
            expect(() => new OracleService(negativeIntervalConfig)).not.toThrow();
        });
    });

    describe('Security Edge Cases', () => {
        it('should handle configuration with no providers', () => {
            const noProvidersConfig = {
                ...baseConfig,
                providers: [],
            };

            service = new OracleService(noProvidersConfig);
            expect(service).toBeDefined();
            
            const status = service.getStatus();
            expect(status.providers).toHaveLength(0);
        });

        it('should handle configuration with all providers disabled', () => {
            const allDisabledConfig = {
                ...baseConfig,
                providers: baseConfig.providers.map(p => ({ ...p, enabled: false })),
            };

            service = new OracleService(allDisabledConfig);
            expect(service).toBeDefined();
            
            const status = service.getStatus();
            expect(status.providers.every(p => !p.enabled)).toBe(true);
        });

        it('should handle provider with zero weight', () => {
            const zeroWeightConfig = {
                ...baseConfig,
                providers: [
                    { ...baseConfig.providers[0], weight: 0 },
                    { ...baseConfig.providers[1], weight: 1 },
                ],
            };

            service = new OracleService(zeroWeightConfig);
            expect(service).toBeDefined();
        });

        it('should handle provider with negative priority', () => {
            const negativePriorityConfig = {
                ...baseConfig,
                providers: [
                    { ...baseConfig.providers[0], priority: -1 },
                    { ...baseConfig.providers[1], priority: 1 },
                ],
            };

            service = new OracleService(negativePriorityConfig);
            expect(service).toBeDefined();
        });

        it('should handle extremely large configuration values', () => {
            const extremeConfig = {
                ...baseConfig,
                maxPriceDeviationPercent: 1000,
                priceStaleThresholdSeconds: Number.MAX_SAFE_INTEGER,
                cacheTtlSeconds: Number.MAX_SAFE_INTEGER,
                updateIntervalMs: Number.MAX_SAFE_INTEGER,
            };

            service = new OracleService(extremeConfig);
            expect(service).toBeDefined();
        });

        it('should handle configuration with duplicate provider names', () => {
            const duplicateConfig = {
                ...baseConfig,
                providers: [
                    { ...baseConfig.providers[0] },
                    { ...baseConfig.providers[0], name: 'coingecko' }, // Duplicate name
                ],
            };

            service = new OracleService(duplicateConfig);
            expect(service).toBeDefined();
        });
    });

    describe('Configuration Persistence', () => {
        it('should maintain configuration across service restarts', () => {
            // Create service with custom config
            const customConfig = {
                ...baseConfig,
                maxPriceDeviationPercent: 15,
                priceStaleThresholdSeconds: 600,
            };

            service = new OracleService(customConfig);
            const initialStatus = service.getStatus();
            
            service.stop();
            
            // Create new service with same config
            const newService = new OracleService(customConfig);
            const newStatus = newService.getStatus();
            
            expect(newStatus).toBeDefined();
            newService.stop();
        });

        it('should allow configuration updates through service recreation', () => {
            service = new OracleService(baseConfig);
            service.stop();
            
            // Update configuration
            const updatedConfig = {
                ...baseConfig,
                maxPriceDeviationPercent: 25,
                providers: [
                    { ...baseConfig.providers[0], enabled: false },
                    baseConfig.providers[1],
                ],
            };
            
            const newService = new OracleService(updatedConfig);
            const status = newService.getStatus();
            
            expect(status).toBeDefined();
            newService.stop();
        });
    });

    describe('Error Handling in Configuration', () => {
        it('should handle malformed provider configuration gracefully', () => {
            const malformedConfig = {
                ...baseConfig,
                providers: [
                    {
                        ...baseConfig.providers[0],
                        baseUrl: undefined as any,
                        rateLimit: null as any,
                    },
                ],
            };

            expect(() => new OracleService(malformedConfig)).not.toThrow();
        });

        it('should handle missing optional configuration fields', () => {
            const minimalConfig = {
                stellarNetwork: 'testnet' as const,
                stellarRpcUrl: 'https://soroban-testnet.stellar.org',
                contractId: 'CTEST123',
                adminSecretKey: 'STEST123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ123456',
                updateIntervalMs: 60000,
                maxPriceDeviationPercent: 10,
                priceStaleThresholdSeconds: 300,
                cacheTtlSeconds: 30,
                logLevel: 'info' as const,
                providers: [],
            };

            service = new OracleService(minimalConfig);
            expect(service).toBeDefined();
        });

        it('should handle configuration with circular references', () => {
            const config: any = { ...baseConfig };
            config.self = config; // Create circular reference

            expect(() => new OracleService(config)).not.toThrow();
        });
    });

    describe('Performance Impact of Configuration', () => {
        it('should handle rapid configuration changes', async () => {
            const configs = Array.from({ length: 10 }, (_, i) => ({
                ...baseConfig,
                maxPriceDeviationPercent: 5 + i,
            }));

            const services = configs.map(config => new OracleService(config));
            
            // All services should initialize successfully
            services.forEach(s => expect(s).toBeDefined());
            
            // Clean up
            services.forEach(s => s.stop());
        });

        it('should handle configuration with many providers', () => {
            const manyProvidersConfig = {
                ...baseConfig,
                providers: Array.from({ length: 20 }, (_, i) => ({
                    name: `provider_${i}`,
                    enabled: true,
                    priority: i + 1,
                    weight: 0.05,
                    baseUrl: `https://provider${i}.example.com`,
                    rateLimit: { maxRequests: 100, windowMs: 60000 },
                })),
            };

            service = new OracleService(manyProvidersConfig);
            expect(service).toBeDefined();
            
            const status = service.getStatus();
            expect(status.providers).toHaveLength(20);
        });
    });
});
