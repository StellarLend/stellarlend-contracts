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
        if (!rawBody) {
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

        const body = payload as {
            validatorConfig?: Partial<ValidatorConfig>;
            bounds?: Record<string, { minPrice: number; maxPrice: number }>;
        };

        if (!body.validatorConfig && !body.bounds) {
            this.sendResponse(res, 400, {
                error: 'Payload must include validatorConfig or bounds',
            });
            return;
        }

        const bounds = this.normalizeBounds(body.bounds ?? {});
        if (body.bounds && bounds instanceof Error) {
            this.sendResponse(res, 400, { error: bounds.message });
            return;
        }

        try {
            this.validator.reloadConfig(body.validatorConfig ?? {}, bounds as Record<string, { minPrice: number; maxPrice: number }>);
            this.sendResponse(res, 200, {
                status: 'ok',
                updatedBounds: Object.keys(bounds as Record<string, { minPrice: number; maxPrice: number }>),
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
