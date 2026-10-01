/**
 * Logger Utility
 *
 * Centralized logging using Winston with configurable levels
 * and structured output for the Oracle Service.
 */

import { createHash } from 'crypto';
import winston from 'winston';

const { combine, timestamp, printf, colorize, errors } = winston.format;

const replacer = (_key: string, value: unknown) =>
    typeof value === 'bigint' ? value.toString() : value;

/**
 * JSON replacer that keeps logging total.
 *
 * Prices, sequence numbers and idempotency keys are bigints, which
 * `JSON.stringify` throws on by default. A log call must never be the reason a
 * request fails, so bigints are rendered as decimal strings, `BigInt` instances
 * likewise, and cycles are collapsed to `"[Circular]"`.
 */
function safeReplacer(_key: string, value: unknown): unknown {
    if (typeof value === 'bigint') {
        return value.toString();
    }
    if (value instanceof Error) {
        return { name: value.name, message: value.message, stack: value.stack };
    }
    return value;
}

/**
 * Stringify log metadata without ever throwing.
 */
function stringifyMeta(meta: unknown): string {
    const seen = new WeakSet<object>();
    try {
        return JSON.stringify(meta, (key, value: unknown) => {
            if (typeof value === 'object' && value !== null) {
                if (seen.has(value)) {
                    return '[Circular]';
                }
                seen.add(value);
            }
            return safeReplacer(key, value);
        });
    } catch {
        return '[unserializable]';
    }
}

/**
 * Custom log format for console output
 */
const consoleFormat = printf(({ level, message, timestamp, ...meta }) => {
    const metaStr = Object.keys(meta).length ? ` ${stringifyMeta(meta)}` : '';
    return `${timestamp} [${level}]: ${message}${metaStr}`;
});

/**
 * Custom log format for JSON output (production)
 */
const jsonFormat = printf(({ level, message, timestamp, ...meta }) => {
    return stringifyMeta({
        timestamp,
        level,
        message,
        ...meta,
    });
});

/**
 * Create a configured logger instance
 */
export function createLogger(level: string = 'info', useJson: boolean = false) {
    return winston.createLogger({
        level,
        format: combine(
            errors({ stack: true }),
            timestamp({ format: 'YYYY-MM-DD HH:mm:ss.SSS' }),
        ),
        transports: [
            new winston.transports.Console({
                format: combine(
                    useJson ? jsonFormat : combine(colorize(), consoleFormat),
                ),
            }),
        ],
    });
}

/**
 * Default logger instance (can be reconfigured at runtime)
 */
export let logger = createLogger('info');

/**
 * Configure the global logger with new settings
 */
export function configureLogger(level: string, useJson: boolean = false) {
    logger = createLogger(level, useJson);
}

/**
 * Log with additional context for price operations
 */
export function logPriceUpdate(
    asset: string,
    price: bigint,
    source: string,
    success: boolean,
    details?: Record<string, unknown>,
) {
    const logData = {
        ...details,
        asset,
        price: price.toString(),
        source,
        success,
    };

    if (success) {
        logger.info('Price update', logData);
    } else {
        logger.error('Price update failed', logData);
    }
}

/**
 * Returns a short SHA-256 prefix of a public key for safe logging.
 * The full key is never stored or emitted; only the first 8 hex characters
 * of the digest are used, which is enough for operator correlation without
 * exposing operational metadata in shared log aggregators.
 *
 * Format: "sha256:<first-8-hex-chars>"   e.g. "sha256:a1b2c3d4"
 */
export function hashPublicKey(pubkey: string): string {
    const hash = createHash('sha256').update(pubkey).digest('hex');
    return `sha256:${hash.slice(0, 8)}`;
}

/**
 * Log provider health status
 */
export function logProviderHealth(
    provider: string,
    healthy: boolean,
    latencyMs?: number,
    error?: string,
) {
    const logData = {
        provider,
        healthy,
        latencyMs,
        error,
    };

    if (healthy) {
        logger.debug('Provider health check', logData);
    } else {
        logger.warn('Provider unhealthy', logData);
    }
}
