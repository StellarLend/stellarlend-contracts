/**
 * AdminServer — timestamp-bound signatures and replay-protection tests.
 *
 * Covers every case required by issue #2423:
 *   - Valid request -> 200
 *   - Same request replayed -> 409
 *   - Timestamp 10 minutes old -> 401
 *   - Body altered after signing -> 403
 *   - Missing x-timestamp -> 401
 *   - Missing x-signature -> 401
 *
 * All tests are deterministic: no network, no filesystem, no wall-clock sleeps.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import http from 'node:http';
import { AdminServer, signRequest } from '../src/services/admin-server.js';
import type { AdminServerOptions } from '../src/services/admin-server.js';

// ---------------------------------------------------------------------------
// Minimal PriceValidator stub
// ---------------------------------------------------------------------------

function makeValidator() {
    return {
        reloadConfig: vi.fn(),
        validate: vi.fn(),
    };
}

// ---------------------------------------------------------------------------
// HTTP helper: send a raw request and collect the response
// ---------------------------------------------------------------------------

interface RequestOptions {
    method?: string;
    path?: string;
    headers?: Record<string, string>;
    body?: string;
}

function sendRequest(port: number, opts: RequestOptions = {}): Promise<{ status: number; body: string }> {
    return new Promise((resolve, reject) => {
        const bodyStr = opts.body ?? '';
        const req = http.request(
            {
                hostname: '127.0.0.1',
                port,
                path: opts.path ?? '/reload-config',
                method: opts.method ?? 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    'Content-Length': Buffer.byteLength(bodyStr),
                    ...opts.headers,
                },
            },
            (res) => {
                let data = '';
                res.on('data', (chunk: Buffer) => { data += chunk.toString(); });
                res.on('end', () => resolve({ status: res.statusCode ?? 0, body: data }));
            },
        );
        req.on('error', reject);
        req.end(bodyStr);
    });
}

// ---------------------------------------------------------------------------
// Helpers that build a correctly-signed request
// ---------------------------------------------------------------------------

const SECRET = 'test-hmac-secret';
const VALID_BODY = JSON.stringify({ bounds: { XLM: { minPrice: 0.1, maxPrice: 1_000_000 } } });

function validHeaders(body = VALID_BODY, tsOverride?: number): Record<string, string> {
    const ts = tsOverride ?? Date.now();
    return {
        'x-timestamp': String(ts),
        'x-signature': signRequest(SECRET, ts, body),
    };
}

// ---------------------------------------------------------------------------
// Suite
// ---------------------------------------------------------------------------

describe('AdminServer — timestamp & replay protection', () => {
    let server: AdminServer;
    let validator: ReturnType<typeof makeValidator>;
    let port: number;

    beforeEach(async () => {
        // Pick an ephemeral port by binding to 0 first so parallel test runs
        // cannot collide.
        port = await new Promise<number>((res) => {
            const tmp = http.createServer();
            tmp.listen(0, '127.0.0.1', () => {
                const addr = tmp.address() as { port: number };
                tmp.close(() => res(addr.port));
            });
        });

        validator = makeValidator();
        const options: AdminServerOptions = {
            port,
            hmacSecret: SECRET,
            validator: validator as never,
        };
        server = new AdminServer(options);
        await server.start();
    });

    afterEach(async () => {
        await server.stop();
        vi.restoreAllMocks();
    });

    // -----------------------------------------------------------------------
    // signRequest helper
    // -----------------------------------------------------------------------

    describe('signRequest()', () => {
        it('returns a 64-character lowercase hex string', () => {
            const sig = signRequest(SECRET, 1_700_000_000_000, VALID_BODY);
            expect(sig).toMatch(/^[0-9a-f]{64}$/);
        });

        it('produces different signatures for different timestamps', () => {
            const ts = 1_700_000_000_000;
            expect(signRequest(SECRET, ts, VALID_BODY)).not.toBe(signRequest(SECRET, ts + 1, VALID_BODY));
        });

        it('produces different signatures for different bodies', () => {
            const ts = Date.now();
            expect(signRequest(SECRET, ts, '{"a":1}')).not.toBe(signRequest(SECRET, ts, '{"a":2}'));
        });

        it('is deterministic for the same inputs', () => {
            const ts = 1_700_000_000_000;
            expect(signRequest(SECRET, ts, VALID_BODY)).toBe(signRequest(SECRET, ts, VALID_BODY));
        });
    });

    // -----------------------------------------------------------------------
    // Valid request -> 200
    // -----------------------------------------------------------------------

    it('returns 200 for a correctly signed, in-window request', async () => {
        const { status } = await sendRequest(port, {
            headers: validHeaders(),
            body: VALID_BODY,
        });
        expect(status).toBe(200);
        expect(validator.reloadConfig).toHaveBeenCalledOnce();
    });

    // -----------------------------------------------------------------------
    // Replay -> 409
    // -----------------------------------------------------------------------

    it('returns 409 when the same signed request is sent a second time', async () => {
        const headers = validHeaders();

        const first = await sendRequest(port, { headers, body: VALID_BODY });
        expect(first.status).toBe(200);

        const second = await sendRequest(port, { headers, body: VALID_BODY });
        expect(second.status).toBe(409);
        expect(JSON.parse(second.body)).toMatchObject({ error: expect.stringMatching(/replay/i) });
        // reloadConfig must NOT be called a second time.
        expect(validator.reloadConfig).toHaveBeenCalledOnce();
    });

    // -----------------------------------------------------------------------
    // Stale timestamp -> 401
    // -----------------------------------------------------------------------

    it('returns 401 when the timestamp is 10 minutes old', async () => {
        const staleTs = Date.now() - 10 * 60 * 1_000;
        const { status, body } = await sendRequest(port, {
            headers: validHeaders(VALID_BODY, staleTs),
            body: VALID_BODY,
        });
        expect(status).toBe(401);
        expect(JSON.parse(body)).toMatchObject({ error: expect.stringMatching(/timestamp/i) });
    });

    it('returns 401 when the timestamp is 10 minutes in the future', async () => {
        const futureTs = Date.now() + 10 * 60 * 1_000;
        const { status } = await sendRequest(port, {
            headers: validHeaders(VALID_BODY, futureTs),
            body: VALID_BODY,
        });
        expect(status).toBe(401);
    });

    // -----------------------------------------------------------------------
    // Body altered after signing -> 403
    // -----------------------------------------------------------------------

    it('returns 403 when the body is altered after the signature is computed', async () => {
        const ts = Date.now();
        const signature = signRequest(SECRET, ts, VALID_BODY);
        const alteredBody = JSON.stringify({ bounds: { XLM: { minPrice: 0.01, maxPrice: 999 } } });

        const { status } = await sendRequest(port, {
            headers: { 'x-timestamp': String(ts), 'x-signature': signature },
            body: alteredBody,
        });
        expect(status).toBe(403);
    });

    // -----------------------------------------------------------------------
    // Missing headers -> 401
    // -----------------------------------------------------------------------

    it('returns 401 when x-timestamp is missing', async () => {
        const ts = Date.now();
        const { status } = await sendRequest(port, {
            headers: { 'x-signature': signRequest(SECRET, ts, VALID_BODY) },
            body: VALID_BODY,
        });
        expect(status).toBe(401);
    });

    it('returns 401 when x-signature is missing', async () => {
        const { status } = await sendRequest(port, {
            headers: { 'x-timestamp': String(Date.now()) },
            body: VALID_BODY,
        });
        expect(status).toBe(401);
    });

    it('returns 401 for a non-numeric x-timestamp', async () => {
        const ts = Date.now();
        const { status } = await sendRequest(port, {
            headers: {
                'x-timestamp': 'not-a-number',
                'x-signature': signRequest(SECRET, ts, VALID_BODY),
            },
            body: VALID_BODY,
        });
        expect(status).toBe(401);
    });

    // -----------------------------------------------------------------------
    // Wrong secret -> 403
    // -----------------------------------------------------------------------

    it('returns 403 when signed with the wrong secret', async () => {
        const ts = Date.now();
        const { status } = await sendRequest(port, {
            headers: {
                'x-timestamp': String(ts),
                'x-signature': signRequest('wrong-secret', ts, VALID_BODY),
            },
            body: VALID_BODY,
        });
        expect(status).toBe(403);
    });

    // -----------------------------------------------------------------------
    // Two distinct valid requests succeed independently
    // -----------------------------------------------------------------------

    it('allows two distinct in-window requests (different timestamps)', async () => {
        const ts1 = Date.now();
        const ts2 = ts1 + 1;

        const r1 = await sendRequest(port, {
            headers: validHeaders(VALID_BODY, ts1),
            body: VALID_BODY,
        });
        const r2 = await sendRequest(port, {
            headers: validHeaders(VALID_BODY, ts2),
            body: VALID_BODY,
        });

        expect(r1.status).toBe(200);
        expect(r2.status).toBe(200);
        expect(validator.reloadConfig).toHaveBeenCalledTimes(2);
    });

    // -----------------------------------------------------------------------
    // Unrelated routes are still rejected
    // -----------------------------------------------------------------------

    it('returns 404 for unknown routes', async () => {
        const { status } = await sendRequest(port, { path: '/unknown' });
        expect(status).toBe(404);
    });
});
