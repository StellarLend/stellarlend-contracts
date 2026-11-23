
import crypto from 'node:crypto';
import type { AddressInfo } from 'node:net';
import { describe, expect, it } from 'vitest';
import { AdminServer } from '../src/services/admin-server.js';
import type { PriceValidator } from '../src/services/price-validator.js';

function makeServer(host?: string) {
    return new AdminServer({
        port: 0,
        ...(host ? { host } : {}),
        hmacSecret: 'test-admin-secret',
        validator: {} as PriceValidator,
    });
}

describe('AdminServer security', () => {
    it('binds to loopback by default', async () => {
        const server = makeServer();
        await server.start();

        const httpServer = (server as unknown as { server: { address(): AddressInfo | null } }).server;
        const address = httpServer.address();
        expect(address?.address).toBe('127.0.0.1');

        await server.stop();
    });

    it('accepts lowercase and uppercase 32-byte hex HMAC signatures', () => {
        const server = makeServer();
        const body = Buffer.from('{"bounds":{}}');
        const lowercase = crypto.createHmac('sha256', 'test-admin-secret').update(body).digest('hex');
        const verify = (server as unknown as { verifySignature(body: Buffer, signature: string): boolean }).verifySignature;

        expect(verify.call(server, body, lowercase)).toBe(true);
        expect(verify.call(server, body, lowercase.toUpperCase())).toBe(true);
    });

    it('rejects malformed signatures such as zz', () => {
        const server = makeServer();
        const verify = (server as unknown as { verifySignature(body: Buffer, signature: string): boolean }).verifySignature;
        expect(verify.call(server, Buffer.from('{}'), 'zz')).toBe(false);
    });

    it('rejects malformed signatures such as zz with 403', async () => {
        const server = makeServer();
        await server.start();

        const httpServer = (server as unknown as { server: { address(): AddressInfo | null } }).server;
        const address = httpServer.address();
        if (!address) throw new Error('Admin server has no bound address');

        const response = await fetch(`http://127.0.0.1:${address.port}/reload-config`, {
            method: 'POST',
            headers: {
                'content-type': 'application/json',
                'x-signature': 'zz',
            },
            body: '{}',
        });

        expect(response.status).toBe(403);
        await server.stop();

    });
});
