/**
 * Admin HTTP server for oracle runtime operations.
 */

import http from 'node:http';
import crypto from 'node:crypto';
import type { IncomingMessage, ServerResponse } from 'node:http';
import { z } from 'zod';
import type { PriceValidator, ValidatorConfig } from './price-validator.js';
import { logger } from '../utils/logger.js';
import { isSupportedAsset } from '../config.js';

export interface AdminServerOptions {
    port: number;
    host?: string;
    hmacSecret: string;
    validator: PriceValidator;
}

const validatorConfigSchema = z
    .object({
        maxDeviationPercent: z.number().refine(n => Number.isFinite(n) && n >= 0, {
            message: 'maxDeviationPercent must be a non-negative finite number',
        }).optional(),
        maxStalenessSeconds: z.number().refine(n => Number.isFinite(n) && n >= 0, {
            message: 'maxStalenessSeconds must be a non-negative finite number',
        }).optional(),
        minPrice: z.number().refine(n => Number.isFinite(n) && n > 0, {
            message: 'minPrice must be a positive finite number',
        }).optional(),
        maxPrice: z.number().refine(n => Number.isFinite(n) && n > 0, {
            message: 'maxPrice must be a positive finite number',
        }).optional(),
        maxFallbackStalenessSeconds: z.number().refine(n => Number.isFinite(n) && n >= 0, {
            message: 'maxFallbackStalenessSeconds must be a non-negative finite number',
        }).optional(),
    })
    .strict({ message: 'Unknown fields in validatorConfig are not allowed' });

const boundRangeSchema = z
    .object({
        minPrice: z.number()
            .refine(n => Number.isFinite(n), { message: 'minPrice must be a finite number' })
            .refine(n => n >= 0, { message: 'minPrice must be non-negative' }),
        maxPrice: z.number()
            .refine(n => Number.isFinite(n), { message: 'maxPrice must be a finite number' }),
    })
    .strict({ message: 'Unknown fields in bound range are not allowed' })
    .refine(b => b.maxPrice > b.minPrice, {
        message: 'maxPrice must be greater than minPrice',
        path: ['maxPrice'],
    });

const boundsSchema = z.record(z.string(), boundRangeSchema).superRefine((val, ctx) => {
    for (const asset of Object.keys(val)) {
        if (!isSupportedAsset(asset.toUpperCase())) {
            ctx.addIssue({
                code: z.ZodIssueCode.custom,
                message: `Unsupported asset in bounds: ${asset}`,
                path: [asset],
            });
        }
    }
});

const reloadPayloadSchema = z
    .object({
        validatorConfig: validatorConfigSchema.optional(),
        bounds: boundsSchema.optional(),
    })
    .strict({ message: 'Unknown fields in reload payload are not allowed' })
    .refine(data => Boolean(data.validatorConfig || data.bounds), {
        message: 'Payload must include validatorConfig or bounds',
    });
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
    private readonly host: string;
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
        this.host = options.host ?? '127.0.0.1';
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
            this.server.listen(this.port, this.host, () => {
                logger.info('Admin server listening', { host: this.host, port: this.port });
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

        const rawBody = await this.readBody(req);
        if (!rawBody || rawBody.length === 0) {
            this.sendResponse(res, 400, { error: 'Empty request body' });
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

        const parseResult = reloadPayloadSchema.safeParse(payload);
        if (!parseResult.success) {
            const firstIssue = parseResult.error.issues[0];
            const path = firstIssue.path.join('.');
            const errorMessage = path ? `${path}: ${firstIssue.message}` : firstIssue.message;
            this.sendResponse(res, 400, { error: errorMessage, details: parseResult.error.issues });
            return;
        }

        const normalizedBounds: Record<string, { minPrice: number; maxPrice: number }> = {};
        if (parseResult.data.bounds) {
            for (const [asset, range] of Object.entries(parseResult.data.bounds)) {
                normalizedBounds[asset.toUpperCase()] = {
                    minPrice: range.minPrice,
                    maxPrice: range.maxPrice,
                };
            }
        }

        try {
            this.validator.reloadConfig(
                parseResult.data.validatorConfig ?? {},
                normalizedBounds
            );
            this.sendResponse(res, 200, {
                status: 'ok',
                updatedBounds: Object.keys(normalizedBounds),
            });
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
            if (!/^[0-9a-fA-F]{64}$/.test(signature)) {
                return false;
            }

            const expectedBuffer = crypto
                .createHmac('sha256', this.hmacSecret)
                .update(body)
                .digest();
            const signatureBuffer = Buffer.from(signature, 'hex');

            if (signatureBuffer.length !== 32 || expectedBuffer.length !== 32) {

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

    private sendResponse(res: ServerResponse, statusCode: number, body: unknown) {
        res.writeHead(statusCode, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify(body));
    }
}
