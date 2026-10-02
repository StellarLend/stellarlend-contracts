import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
    configureLogger,
    createLogger,
    hashPublicKey,
    logPriceUpdate,
    logProviderHealth,
    logger,
} from '../src/utils/logger.js';

describe('logger utility', () => {
    beforeEach(() => {
        configureLogger('silly');
    });

    afterEach(() => {
        vi.restoreAllMocks();
        configureLogger('info');
    });

    it('creates loggers with the requested level', () => {
        expect(createLogger('error').level).toBe('error');
    });

    it('replaces the global logger when its configuration changes', () => {
        configureLogger('error');
        expect(logger.level).toBe('error');

        configureLogger('debug');
        expect(logger.level).toBe('debug');
    });

    it('logs successful price updates with exact bigint values and protected fields', () => {
        const info = vi.spyOn(logger, 'info');

        logPriceUpdate('XLM', 9_007_199_254_740_993n, 'primary', true, {
            attempt: 2,
            asset: 'spoofed',
            price: 'spoofed',
            source: 'spoofed',
            success: false,
        });

        expect(info).toHaveBeenCalledWith('Price update', {
            asset: 'XLM',
            price: '9007199254740993',
            source: 'primary',
            success: true,
            attempt: 2,
        });
    });

    it('routes failed price updates to error logging', () => {
        const error = vi.spyOn(logger, 'error');
        const info = vi.spyOn(logger, 'info');

        logPriceUpdate('USDC', 0n, 'fallback', false, { reason: 'unavailable' });

        expect(error).toHaveBeenCalledWith('Price update failed', {
            asset: 'USDC',
            price: '0',
            source: 'fallback',
            success: false,
            reason: 'unavailable',
        });
        expect(info).not.toHaveBeenCalled();
    });

    it('routes provider health to debug or warning with boundary latency values', () => {
        const debug = vi.spyOn(logger, 'debug');
        const warn = vi.spyOn(logger, 'warn');

        logProviderHealth('rpc-a', true, 0);
        logProviderHealth('rpc-b', false, Number.MAX_SAFE_INTEGER, 'timeout');

        expect(debug).toHaveBeenCalledWith('Provider health check', {
            provider: 'rpc-a',
            healthy: true,
            latencyMs: 0,
            error: undefined,
        });
        expect(warn).toHaveBeenCalledWith('Provider unhealthy', {
            provider: 'rpc-b',
            healthy: false,
            latencyMs: Number.MAX_SAFE_INTEGER,
            error: 'timeout',
        });
    });

    it('hashes public keys deterministically without returning the raw key', () => {
        const publicKey = 'GABC123456789';
        const digest = hashPublicKey(publicKey);

        expect(digest).toMatch(/^sha256:[0-9a-f]{8}$/);
        expect(digest).toBe(hashPublicKey(publicKey));
        expect(digest).not.toContain(publicKey);
    });

    it('handles an empty public key as a deterministic boundary input', () => {
        expect(hashPublicKey('')).toBe('sha256:e3b0c442');
    });
});
