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

export class AdminServer {
    private server?: http.Server;
    private readonly port: number;
    private readonly hmacSecret: string;
    private readonly validator: PriceValidator;

    constructor(options: AdminServerOptions) {
        this.port = options.port;
        this.hmacSecret = options.hmacSecret;
        this.validator = options.validator;
    }

    start(): Promise<void> {
        return new Promise((resolve, reject) => {
            this.server = http.createServer(this.requestHandler.bind(this));

            this.server.once('error', reject);
            this.server.listen(this.port, () => {
                logger.info('Admin server listening', { port: this.port });
                resolve();
            });
        });
    }

    stop(): Promise<void> {
        return new Promise((resolve, reject) => {
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

    private async requestHandler(req: IncomingMessage, res: ServerResponse) {
        const { url, method, headers } = req;

        if (url !== '/reload-config' || method !== 'POST') {
            res.writeHead(404, { 'Content-Type': 'application/json' });
            res.end(JSON.stringify({ error: 'Not found' }));
            return;
        }

        const signature = headers['x-signature'];
        if (!signature || Array.isArray(signature)) {
            this.sendResponse(res, 401, { error: 'Missing signature header' });
            return;
        }

        const rawBody = await this.readBody(req);
        if (!rawBody || rawBody.length === 0) {
            this.sendResponse(res, 400, { error: 'Empty request body' });
            return;
        }

        if (!this.verifySignature(rawBody, signature)) {
            this.sendResponse(res, 403, { error: 'Invalid signature' });
            return;
        }

        let payload: unknown;
        try {
            payload = JSON.parse(rawBody.toString('utf8'));
        } catch (error) {
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
        } catch (error) {
            logger.error('Failed to reload validator config', { error });
            this.sendResponse(res, 500, { error: 'Unable to reload config' });
        }
    }

    private readBody(req: IncomingMessage): Promise<Buffer> {
        return new Promise((resolve, reject) => {
            const chunks: Buffer[] = [];

            req.on('data', (chunk) => chunks.push(Buffer.from(chunk)));
            req.on('end', () => resolve(Buffer.concat(chunks)));
            req.on('error', reject);
        });
    }

    private verifySignature(body: Buffer, signature: string): boolean {
        try {
            const expected = crypto
                .createHmac('sha256', this.hmacSecret)
                .update(body)
                .digest('hex');

            const signatureBuffer = Buffer.from(signature, 'utf8');
            const expectedBuffer = Buffer.from(expected, 'utf8');

            if (signatureBuffer.length !== expectedBuffer.length) {
                return false;
            }

            return crypto.timingSafeEqual(signatureBuffer, expectedBuffer);
        } catch (error) {
            logger.error('Signature verification failed', { error });
            return false;
        }
    }

    private sendResponse(res: ServerResponse, statusCode: number, body: unknown) {
        res.writeHead(statusCode, { 'Content-Type': 'application/json' });
        res.end(JSON.stringify(body));
    }
}
