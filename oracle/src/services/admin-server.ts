/**
 * Admin HTTP server for oracle runtime operations.
 */

import http from 'node:http';
import crypto from 'node:crypto';
import type { IncomingMessage, ServerResponse } from 'node:http';
import type { PriceValidator, ValidatorConfig } from './price-validator.js';
import { logger } from '../utils/logger.js';
import { isSupportedAsset } from '../config.js';

export interface AdminServerOptions {
    port: number;
    hmacSecret: string;
    validator: PriceValidator;
}

const MAX_BODY_BYTES = 64 * 1024;
const REQUEST_TIMEOUT_MS = 10_000;
const HEADERS_TIMEOUT_MS = 10_000;

/** Acceptable clock skew in milliseconds (±5 minutes). */
const TIMESTAMP_WINDOW_MS = 5 * 60 * 1_000;

/** How often the replay cache is pruned of expired entries. */
const REPLAY_CACHE_PRUNE_INTERVAL_MS = 60_000;

class AdminBodyError extends Error {
    constructor(message: string, public readonly statusCode: number) {
        super(message);
        this.name = 'AdminBodyError';
    }
}

/**
 * Compute the HMAC-SHA256 signature for an admin request.
 *
 * The signed message is `${timestampMs}.${rawBody}`, mirroring the pattern
 * used by the existing middleware in `api/src/middleware/auth.ts`.
 *
 * @param secret      - The HMAC secret (plain string).
 * @param timestampMs - The `x-timestamp` value that will be sent with the request.
 * @param rawBody     - The raw request body bytes (or string).
 * @returns Lowercase hex-encoded HMAC-SHA256 digest.
 */
export function signRequest(secret: string, timestampMs: number, rawBody: Buffer | string): string {
    const body = Buffer.isBuffer(rawBody) ? rawBody.toString('utf8') : rawBody;
    return crypto
        .createHmac('sha256', secret)
        .update(`${timestampMs}.${body}`)
        .digest('hex');
}

export class AdminServer {
    private server?: http.Server;
    private readonly port: number;
    private readonly hmacSecret: string;
    private readonly validator: PriceValidator;

    /**
     * In-memory replay cache: maps a recently-seen signature to the timestamp
     * at which its validity window expires. Entries are pruned periodically.
     */
    private readonly replayCache = new Map<string, number>();
    private pruneTimer?: NodeJS.Timeout;

    constructor(options: AdminServerOptions) {
        this.port = options.port;
        this.hmacSecret = options.hmacSecret;
        this.validator = options.validator;
    }

    start(): Promise<void> {
        return new Promise((resolve, reject) => {
            this.server = http.createServer((req, res) => {
                void this.requestHandler(req, res);
            });
            this.server.requestTimeout = REQUEST_TIMEOUT_MS;
            this.server.headersTimeout = HEADERS_TIMEOUT_MS;

            this.pruneTimer = setInterval(() => {
                this.pruneReplayCache();
            }, REPLAY_CACHE_PRUNE_INTERVAL_MS);
            // Allow the timer to be garbage-collected without keeping the process alive.
            this.pruneTimer.unref();

            this.server.once('error', reject);
            this.server.listen(this.port, () => {
                logger.info('Admin server listening', { port: this.port });
                resolve();
            });
        });
    }

    stop(): Promise<void> {
        return new Promise((resolve, reject) => {
            if (this.pruneTimer) {
                clearInterval(this.pruneTimer);
                this.pruneTimer = undefined;
            }

            if (!this.server) {
                resolve();
                return;
            }

            this.server.close((error) => {
                if (error) {
                    reject(error);
                } else {
                    logger.info('Admin server stopped');
                    resolve();
                }
            });
        });
    }

    private async requestHandler(req: IncomingMessage, res: ServerResponse): Promise<void> {
        try {
            const { url, method, headers } = req;

            if (url !== '/reload-config' || method !== 'POST') {
                res.writeHead(404, { 'Content-Type': 'application/json' });
                res.end(JSON.stringify({ error: 'Not found' }));
                return;
            }

            const contentType = headers['content-type'];
            if (typeof contentType === 'string' && !contentType.toLowerCase().startsWith('application/json')) {
                this.sendResponse(res, 415, { error: 'Content-Type must be application/json' });
                return;
            }

            // --- Timestamp check (must come before signature) ---
            const timestampHeader = headers['x-timestamp'];
            if (!timestampHeader || Array.isArray(timestampHeader)) {
                this.sendResponse(res, 401, { error: 'Missing x-timestamp header' });
                return;
            }

            const timestampMs = Number(timestampHeader);
            if (!Number.isFinite(timestampMs) || timestampMs <= 0) {
                this.sendResponse(res, 401, { error: 'Invalid x-timestamp value' });
                return;
            }

            const now = Date.now();
            if (Math.abs(now - timestampMs) > TIMESTAMP_WINDOW_MS) {
                this.sendResponse(res, 401, { error: 'Request timestamp outside acceptable window' });
                return;
            }

            // --- Signature check ---
            const signature = headers['x-signature'];
            if (!signature || Array.isArray(signature)) {
                this.sendResponse(res, 401, { error: 'Missing signature header' });
                return;
            }

            const rawBody = await this.readBody(req);
            if (!rawBody.length) {
                this.sendResponse(res, 400, { error: 'Empty request body' });
                return;
            }

            if (!this.verifySignature(rawBody, timestampMs, signature)) {
                this.sendResponse(res, 403, { error: 'Invalid signature' });
                return;
            }

            // --- Replay check (after signature so we only cache verified sigs) ---
            if (this.replayCache.has(signature)) {
                this.sendResponse(res, 409, { error: 'Duplicate request (replay rejected)' });
                return;
            }

            // Record the signature; expire it when its timestamp window closes.
            const expiresAt = timestampMs + TIMESTAMP_WINDOW_MS;
            this.replayCache.set(signature, expiresAt);

            // --- Parse and apply payload ---
            let payload: unknown;
            try {
                payload = JSON.parse(rawBody.toString('utf8'));
            } catch {
                this.sendResponse(res, 400, { error: 'Invalid JSON body' });
                return;
            }

            if (typeof payload !== 'object' || payload === null) {
                this.sendResponse(res, 400, { error: 'Payload must be an object' });
                return;
            }

            const body = payload as {
                validatorConfig?: Partial<ValidatorConfig>;
                bounds?: Record<string, { minPrice: number; maxPrice: number }>;
            };

            if (!body.validatorConfig && !body.bounds) {
                this.sendResponse(res, 400, { error: 'Payload must include validatorConfig or bounds' });
                return;
            }

            const bounds = this.normalizeBounds(body.bounds ?? {});
            if (bounds instanceof Error) {
                this.sendResponse(res, 400, { error: bounds.message });
                return;
            }

            try {
                this.validator.reloadConfig(body.validatorConfig ?? {}, bounds);
                this.sendResponse(res, 200, { status: 'ok', updatedBounds: Object.keys(bounds) });
            } catch (error) {
                logger.error('Failed to reload validator config', { error });
                this.sendResponse(res, 500, { error: 'Unable to reload config' });
            }
        } catch (error) {
            if (error instanceof AdminBodyError) {
                logger.warn('Admin request body rejected', { statusCode: error.statusCode, error: error.message });
                if (!res.headersSent) this.sendResponse(res, error.statusCode, { error: error.message });
                return;
            }
            logger.warn('Unexpected admin handler error', { error });
            if (!res.headersSent) this.sendResponse(res, 500, { error: 'Internal server error' });
        }
    }

    private readBody(req: IncomingMessage): Promise<Buffer> {
        return new Promise((resolve, reject) => {
            const chunks: Buffer[] = [];
            let receivedBytes = 0;

            const timeout = setTimeout(() => {
                const error = new AdminBodyError('Request timed out', 408);
                req.destroy();
                reject(error);
            }, REQUEST_TIMEOUT_MS);

            const cleanup = () => {
                clearTimeout(timeout);
                req.off('data', onData);
                req.off('end', onEnd);
                req.off('error', onError);
                req.off('aborted', onAborted);
            };

            const onData = (chunk: Buffer | string) => {
                const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
                receivedBytes += buffer.length;
                if (receivedBytes > MAX_BODY_BYTES) {
                    cleanup();
                    const error = new AdminBodyError('Request body too large', 413);
                    req.destroy();
                    reject(error);
                    return;
                }
                chunks.push(buffer);
            };

            const onEnd = () => {
                cleanup();
                resolve(Buffer.concat(chunks));
            };

            const onError = (error: Error) => {
                cleanup();
                reject(new AdminBodyError(error.message || 'Request read error', 400));
            };

            const onAborted = () => {
                cleanup();
                reject(new AdminBodyError('Request aborted', 400));
            };

            req.on('data', onData);
            req.on('end', onEnd);
            req.on('error', onError);
            req.on('aborted', onAborted);
        });
    }

    private verifySignature(body: Buffer, timestampMs: number, signature: string): boolean {
        try {
            const expected = signRequest(this.hmacSecret, timestampMs, body);

            const signatureBuffer = Buffer.from(signature, 'hex');
            const expectedBuffer = Buffer.from(expected, 'hex');

            if (signatureBuffer.length !== expectedBuffer.length || expectedBuffer.length === 0) {
                return false;
            }

            return crypto.timingSafeEqual(signatureBuffer, expectedBuffer);
        } catch (error) {
            logger.error('Signature verification failed', { error });
            return false;
        }
    }

    private pruneReplayCache(): void {
        const now = Date.now();
        for (const [sig, expiresAt] of this.replayCache) {
            if (now >= expiresAt) {
                this.replayCache.delete(sig);
            }
        }
    }

    private normalizeBounds(
        bounds: Record<string, { minPrice: number; maxPrice: number }>,
    ): Record<string, { minPrice: number; maxPrice: number }> | Error {
        const normalized: Record<string, { minPrice: number; maxPrice: number }> = {};

        for (const [asset, range] of Object.entries(bounds)) {
            const upperAsset = asset.toUpperCase();
            if (!isSupportedAsset(upperAsset)) {
                return new Error(`Unsupported asset in bounds: ${asset}`);
            }

            if (
                typeof range.minPrice !== 'number' ||
                typeof range.maxPrice !== 'number' ||
                Number.isNaN(range.minPrice) ||
                Number.isNaN(range.maxPrice)
            ) {
                return new Error('Bounds must contain numeric minPrice and maxPrice values');
            }

            if (range.maxPrice <= range.minPrice) {
                return new Error(
                    `Invalid bounds for ${upperAsset}: maxPrice must be greater than minPrice`,
                );
            }

            normalized[upperAsset] = {
                minPrice: range.minPrice,
                maxPrice: range.maxPrice,
            };
        }

        return normalized;
    }

    private sendResponse(res: ServerResponse, statusCode: number, body: unknown) {
        res.writeHead(statusCode, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify(body));
    }
}
