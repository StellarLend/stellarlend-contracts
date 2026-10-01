import crypto from 'crypto';
import request from 'supertest';

const mockProcessHook = jest.fn((_request: unknown, response: any) =>
  response.status(200).json({ success: true, message: 'Hook authenticated' })
);
const mockDeposit = jest.fn((_request: unknown, response: any) =>
  response.status(200).json({ success: true, transactionHash: 'tx-hash' })
);
const mockBorrow = jest.fn((_request: unknown, response: any) =>
  response.status(200).json({ success: true, transactionHash: 'tx-hash' })
);
const mockRepay = jest.fn();
const mockWithdraw = jest.fn();
const mockGetActivity = jest.fn((_request: unknown, response: any) =>
  response.status(200).json({ data: [], pagination: {} })
);
const mockHealthCheck = jest.fn((_request: unknown, response: any) =>
  response.status(200).json({ status: 'healthy', timestamp: '1970-01-01T00:00:00.000Z' })
);
const mockDeepHealthCheck = jest.fn((_request: unknown, response: any) =>
  response.status(200).json({ rpc: true, contract: true, ledger: 1 })
);

// The controller is replaced so the tests exercise `app.ts`'s own wiring
// (parsers, rate limit, 404s, error routing) and never touch the network.
// Every export referenced by `lending.routes.ts` / `health.routes.ts` must be
// present: Express throws at import time if a route is mounted with an
// undefined callback, which is itself a regression this suite guards.
jest.mock('../controllers/lending.controller', () => ({
  processHook: mockProcessHook,
  deposit: mockDeposit,
  borrow: mockBorrow,
  repay: mockRepay,
  withdraw: mockWithdraw,
  getActivity: mockGetActivity,
  healthCheck: mockHealthCheck,
  deepHealthCheck: mockDeepHealthCheck,
}));

const HOOK_SECRET = 'app-regression-hook-secret';
const VALID_USER_ADDRESS = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ';
const VALID_AMOUNT = '1000000';

// `config` snapshots the environment at import time, so the hook secret, a JWT
// secret (to silence the insecure-default audit) and a rate limit high enough
// to keep ordinary assertions from being throttled must all be set before the
// app module is loaded. The rate limit itself is asserted against a dedicated,
// freshly-built instance in the "rate limit" describe block.
process.env.STELLAR_API_HOOK_SECRET = HOOK_SECRET;
process.env.JWT_SECRET = 'app-regression-jwt-secret';
process.env.RATE_LIMIT_MAX_REQUESTS = '100000';

jest.resetModules();
const {
  default: app,
  BODY_LIMIT,
  PARSER_FAILURES,
  RATE_LIMIT_MESSAGE,
  rateLimitKey,
  resolveParserFailure,
} = require('../app');
// Required *after* the reset so this is the very same logger instance the app
// module holds; spying on a pre-reset instance would assert nothing.
const logger = require('../utils/logger').default;

/** Body-parser's `limit: '100kb'` boundary, in bytes. */
const LIMIT_BYTES = 102400;

/** Build a syntactically valid JSON body of exactly `totalBytes` bytes. */
const jsonOfExactSize = (totalBytes: number): string => {
  const prefix = '{"pad":"';
  const suffix = '"}';
  return prefix + 'x'.repeat(totalBytes - prefix.length - suffix.length) + suffix;
};

const sign = (timestamp: string, body: string) =>
  crypto.createHmac('sha256', HOOK_SECRET).update(`${timestamp}.${body}`).digest('hex');

const hookPathFor = (name: string) => `/api/lending/hooks/${name}`;

describe('API app authorization and validation', () => {
  const hookPath = '/api/lending/hooks/indexer';
  const timestamp = Date.now().toString();
  const rawBody = '{ "event": "indexer.write", "data": { "id": "abc123" } }';

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('authenticates hooks against the exact request body bytes', async () => {
    const response = await request(app)
      .post(hookPath)
      .set('Content-Type', 'application/json')
      .set('X-Hook-Timestamp', timestamp)
      .set('X-Hook-Signature', sign(timestamp, rawBody))
      .send(rawBody);

    expect(response.status).toBe(200);
    expect(response.body).toEqual({ success: true, message: 'Hook authenticated' });
    expect(mockProcessHook).toHaveBeenCalledTimes(1);
  });

  it('rejects a hook signature for reserialized rather than raw JSON', async () => {
    const response = await request(app)
      .post(hookPath)
      .set('Content-Type', 'application/json')
      .set('X-Hook-Timestamp', timestamp)
      .set('X-Hook-Signature', sign(timestamp, JSON.stringify(JSON.parse(rawBody))))
      .send(rawBody);

    expect(response.status).toBe(401);
    expect(response.body).toEqual({ success: false, error: 'Invalid hook signature' });
    expect(mockProcessHook).not.toHaveBeenCalled();
  });

  it('returns a client error for malformed JSON without leaking submitted secrets', async () => {
    const response = await request(app)
      .post('/api/lending/deposit')
      .set('Content-Type', 'application/json')
      .send('{ "userSecret": "submitted-secret", invalid }');

    expect(response.status).toBe(400);
    expect(response.body.success).toBe(false);
    expect(JSON.stringify(response.body)).not.toContain('submitted-secret');
  });

  it('rejects invalid lending input before processing the request', async () => {
    const response = await request(app)
      .post('/api/lending/deposit')
      .send({
        userAddress: 'not-a-stellar-address',
        amount: VALID_AMOUNT,
        userSecret: 'submitted-secret',
      });

    expect(response.status).toBe(400);
    expect(response.body.success).toBe(false);
    expect(response.body.error).toContain('valid Stellar');
    expect(JSON.stringify(response.body)).not.toContain('submitted-secret');
    expect(mockDeposit).not.toHaveBeenCalled();
  });
});

describe('app.ts middleware pipeline', () => {
  beforeEach(() => {
    jest.clearAllMocks();
  });

  describe('success path', () => {
    it('serves a valid lending request and preserves the validated body', async () => {
      const response = await request(app).post('/api/lending/deposit').send({
        userAddress: VALID_USER_ADDRESS,
        amount: VALID_AMOUNT,
        userSecret: 'S-submitted-signing-secret',
      });

      expect(response.status).toBe(200);
      expect(response.body).toEqual({ success: true, transactionHash: 'tx-hash' });
      expect(mockDeposit).toHaveBeenCalledTimes(1);

      // The controller must observe the parsed, normalized body — not the raw
      // string — otherwise downstream Stellar calls receive a string.
      const forwarded = mockDeposit.mock.calls[0][0].body;
      expect(forwarded).toEqual({
        userAddress: VALID_USER_ADDRESS,
        amount: VALID_AMOUNT,
        userSecret: 'S-submitted-signing-secret',
      });
    });

    it('serves every mounted route family', async () => {
      const valid = {
        userAddress: VALID_USER_ADDRESS,
        amount: VALID_AMOUNT,
        userSecret: 'S-submitted-signing-secret',
      };

      const responses = await Promise.all([
        request(app).post('/api/lending/borrow').send(valid),
        request(app).get('/api/lending/activity'),
        request(app).get('/api/health'),
        request(app).get('/api/health/healthz'),
      ]);

      expect(responses.map((r) => r.status)).toEqual([200, 200, 200, 200]);
      expect(mockBorrow).toHaveBeenCalledTimes(1);
      expect(mockGetActivity).toHaveBeenCalledTimes(1);
      expect(mockHealthCheck).toHaveBeenCalledTimes(1);
      expect(mockDeepHealthCheck).toHaveBeenCalledTimes(1);
    });

    it('applies helmet security headers and CORS to every response', async () => {
      const response = await request(app).get('/api/health');

      expect(response.status).toBe(200);
      expect(response.headers['x-content-type-options']).toBe('nosniff');
      expect(response.headers['content-security-policy']).toBeDefined();
      expect(response.headers['access-control-allow-origin']).toBe('*');
    });

    it('captures the raw body for urlencoded requests too, not just JSON', async () => {
      // A form-encoded hook signed over the raw form body must authenticate.
      // Without `verify` on the urlencoded parser the middleware would fall back
      // to `JSON.stringify(req.body)` and reject a correctly signed request.
      const formBody = 'event=indexer.write&data=abc123';
      const ts = Date.now().toString();

      const response = await request(app)
        .post(hookPathFor('indexer'))
        .set('Content-Type', 'application/x-www-form-urlencoded')
        .set('X-Hook-Timestamp', ts)
        .set('X-Hook-Signature', sign(ts, formBody))
        .send(formBody);

      expect(response.status).toBe(200);
      expect(mockProcessHook).toHaveBeenCalledTimes(1);
      expect(mockProcessHook.mock.calls[0][0].rawBody).toBe(formBody);
    });
  });

  describe('rejection paths', () => {
    it('rejects a hook with no signature headers and never reaches the controller', async () => {
      const response = await request(app)
        .post(hookPathFor('indexer'))
        .send({ event: 'indexer.write' });

      expect(response.status).toBe(401);
      expect(response.body).toEqual({
        success: false,
        error: 'Hook signature and timestamp headers are required',
      });
      expect(mockProcessHook).not.toHaveBeenCalled();
    });

    it('rejects a hook with a stale timestamp (replay outside the window)', async () => {
      const ts = (Date.now() - 10 * 60 * 1000).toString();

      const response = await request(app)
        .post(hookPathFor('indexer'))
        .send({ event: 'indexer.write' })
        .set('X-Hook-Timestamp', ts)
        .set('X-Hook-Signature', sign(ts, JSON.stringify({ event: 'indexer.write' })));

      expect(response.status).toBe(401);
      expect(response.body.error).toMatch(/timestamp outside allowable window/i);
      expect(mockProcessHook).not.toHaveBeenCalled();
    });

    it('rejects a non-numeric hook timestamp', async () => {
      const response = await request(app)
        .post(hookPathFor('indexer'))
        .send({ event: 'indexer.write' })
        .set('X-Hook-Timestamp', 'not-a-number')
        .set('X-Hook-Signature', 'abcd');

      expect(response.status).toBe(401);
      expect(response.body.error).toMatch(/invalid hook timestamp/i);
      expect(mockProcessHook).not.toHaveBeenCalled();
    });

    it('rejects a tampered body even when the signature is well-formed', async () => {
      const ts = Date.now().toString();
      const signedBody = JSON.stringify({ amount: '1' });
      const tamperedBody = JSON.stringify({ amount: '999999999999' });

      const response = await request(app)
        .post(hookPathFor('indexer'))
        .set('Content-Type', 'application/json')
        .set('X-Hook-Timestamp', ts)
        .set('X-Hook-Signature', sign(ts, signedBody))
        .send(tamperedBody);

      expect(response.status).toBe(401);
      expect(response.body.error).toMatch(/invalid hook signature/i);
      expect(mockProcessHook).not.toHaveBeenCalled();
    });

    it('rejects boundary-invalid lending amounts before the controller runs', async () => {
      for (const amount of ['0', '-1', '1.5', '1e18', String(2n ** 127n), 'not-a-number']) {
        const response = await request(app)
          .post('/api/lending/deposit')
          .send({ userAddress: VALID_USER_ADDRESS, amount, userSecret: 'S-secret' });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      }

      expect(mockDeposit).not.toHaveBeenCalled();
    });

    it('rejects an empty body', async () => {
      const response = await request(app).post('/api/lending/deposit').send({});

      expect(response.status).toBe(400);
      expect(mockDeposit).not.toHaveBeenCalled();
    });
  });

  describe('unknown routes', () => {
    it('returns the same JSON 404 envelope for every unknown path', async () => {
      const paths = [
        '/api/nope',
        '/api',
        '/api/',
        '/api/lending/does-not-exist',
        '/not-an-api-route',
        '/',
      ];

      const responses = await Promise.all(paths.map((p) => request(app).get(p)));

      for (const response of responses) {
        expect(response.status).toBe(404);
        expect(response.headers['content-type']).toMatch(/application\/json/);
        expect(response.body).toEqual({ success: false, error: 'Not Found' });
      }
    });

    it('does not let an unauthenticated caller probe whether a hook path exists', async () => {
      // The hook guard is mounted on the `/hooks` prefix, so it answers before
      // routing. Probing with GET/POST alike must be indistinguishable.
      const [get, post] = await Promise.all([
        request(app).get(hookPathFor('indexer')),
        request(app).post('/api/lending/hooks').send({ event: 'indexer.write' }),
      ]);

      expect(get.status).toBe(401);
      expect(post.status).toBe(401);
      expect(get.body).toEqual(post.body);
      expect(mockProcessHook).not.toHaveBeenCalled();
    });

    it('does not leak whether an /api/ route exists via a different body', async () => {
      const [apiMiss, nonApiMiss] = await Promise.all([
        request(app).post('/api/lending/unknown').send({ a: 1 }),
        request(app).post('/unknown').send({ a: 1 }),
      ]);

      expect(apiMiss.body).toEqual(nonApiMiss.body);
      expect(apiMiss.status).toBe(nonApiMiss.status);
    });
  });

  describe('body size and content boundaries', () => {
    const post = (body: string, contentType = 'application/json') =>
      request(app)
        .post('/api/unknown-endpoint')
        .set('Content-Type', contentType)
        .send(body);

    it('accepts a body of exactly the documented limit', async () => {
      expect(BODY_LIMIT).toBe('100kb');

      const response = await post(jsonOfExactSize(LIMIT_BYTES));

      // Parsed successfully, so routing (not the parser) decides the outcome.
      expect(response.status).toBe(404);
    });

    it('rejects a body one byte over the documented limit', async () => {
      const response = await post(jsonOfExactSize(LIMIT_BYTES + 1));

      expect(response.status).toBe(413);
      expect(response.body).toEqual({ success: false, error: 'Payload too large' });
    });

    it('rejects an oversized body with the same answer on every retry', async () => {
      const body = jsonOfExactSize(LIMIT_BYTES + 1);

      const responses = await Promise.all([post(body), post(body), post(body)]);

      for (const response of responses) {
        expect(response.status).toBe(413);
        expect(response.body).toEqual({ success: false, error: 'Payload too large' });
      }
    });

    it('rejects too many form parameters with 413 rather than 500', async () => {
      const form = Array.from({ length: 1200 }, (_, i) => `p${i}=${i}`).join('&');

      const response = await request(app)
        .post('/api/unknown-endpoint')
        .type('form')
        .send(form);

      expect(response.status).toBe(413);
      expect(response.body).toEqual({ success: false, error: 'Too many parameters' });
    });

    it('rejects an unsupported charset with 415 rather than 500', async () => {
      const response = await request(app)
        .post('/api/unknown-endpoint')
        .set('Content-Type', 'application/json; charset=iso-8859-1')
        .send('{"a":1}');

      expect(response.status).toBe(415);
      expect(response.body).toEqual({ success: false, error: 'Unsupported charset' });
    });

    it('rejects an unsupported content-encoding with 415 rather than 500', async () => {
      const response = await request(app)
        .post('/api/unknown-endpoint')
        .set('Content-Type', 'application/json')
        .set('Content-Encoding', 'bogus-encoding')
        .send('{"a":1}');

      expect(response.status).toBe(415);
      expect(response.body).toEqual({
        success: false,
        error: 'Unsupported content encoding',
      });
    });

    it('maps a malformed JSON body to 400 regardless of content-type parameters', async () => {
      const responses = await Promise.all([
        post('{ "userSecret": "leak-me", invalid }'),
        post('{ "userSecret": "leak-me", invalid }', 'application/json; charset=utf-8'),
      ]);

      for (const response of responses) {
        expect(response.status).toBe(400);
        expect(response.body).toEqual({ success: false, error: 'Invalid JSON body' });
        expect(JSON.stringify(response.body)).not.toContain('leak-me');
      }
    });

    it('ignores a body sent with a content type it does not parse', async () => {
      const response = await request(app)
        .post('/api/unknown-endpoint')
        .set('Content-Type', 'text/plain')
        .send('{ not json at all }');

      // Not a parse failure: an unparsed content type simply routes to the 404.
      expect(response.status).toBe(404);
    });
  });

  describe('parser failure classification (unit)', () => {
    it('resolves every documented body-parser failure type', () => {
      for (const [type, expected] of Object.entries(PARSER_FAILURES)) {
        expect(resolveParserFailure({ type })).toEqual(expected);
      }
    });

    it('returns null for anything that did not come from a body parser', () => {
      // Classification must never guess: an unrecognised failure is handed to
      // the central error handler, which fails closed with a 500.
      expect(resolveParserFailure(undefined)).toBeNull();
      expect(resolveParserFailure(null)).toBeNull();
      expect(resolveParserFailure('entity.too.large')).toBeNull();
      expect(resolveParserFailure(new Error('boom'))).toBeNull();
      expect(resolveParserFailure({ type: 'entity.parse.failed' })).toBeNull();
      expect(resolveParserFailure({ type: 'stream.encoding.set' })).toBeNull();
      expect(resolveParserFailure({ type: 413 })).toBeNull();
    });

    it('ignores inherited object keys so a crafted error cannot forge a status', () => {
      for (const key of ['__proto__', 'constructor', 'toString', 'hasOwnProperty']) {
        expect(resolveParserFailure({ type: key })).toBeNull();
      }
    });

    it('uses only closed-vocabulary messages that cannot echo caller data', () => {
      for (const failure of Object.values(PARSER_FAILURES)) {
        expect(failure.status).toBeGreaterThanOrEqual(400);
        expect(failure.status).toBeLessThan(500);
        expect(failure.success).toBe(false);
        expect(typeof failure.error).toBe('string');
        expect(failure.error.length).toBeGreaterThan(0);
      }
    });
  });

  describe('diagnosability without sensitive data', () => {
    const spyOnLoggers = () => [
      jest.spyOn(logger, 'error').mockImplementation(() => logger),
      jest.spyOn(logger, 'warn').mockImplementation(() => logger),
    ];

    it('never writes a submitted secret or parser text to the log', async () => {
      const spies = spyOnLoggers();

      try {
        await request(app)
          .post('/api/lending/deposit')
          .set('Content-Type', 'application/json')
          .send(
            JSON.stringify({ userSecret: 'OVERSIZED-LEAK-CANARY', pad: 'x'.repeat(LIMIT_BYTES) })
          );

        await request(app)
          .post('/api/lending/deposit')
          .set('Content-Type', 'application/json')
          .send('{ "userSecret": "MALFORMED-LEAK-CANARY", invalid }');

        const logged = JSON.stringify(spies.flatMap((spy) => spy.mock.calls));
        expect(logged).not.toContain('OVERSIZED-LEAK-CANARY');
        expect(logged).not.toContain('MALFORMED-LEAK-CANARY');
        // A parser's own message is never forwarded either: it is derived from
        // the submitted bytes, so its wording is not a stable, non-sensitive
        // diagnostic. The closed-vocabulary `type` carries that role instead.
        expect(logged).not.toContain('request entity too large');
        expect(logged).not.toContain('Expected');
        expect(logged.length).toBeGreaterThan(0);
      } finally {
        spies.forEach((spy) => spy.mockRestore());
      }
    });

    it('records the failure category, status, method, and path so it is diagnosable', async () => {
      const warn = jest.spyOn(logger, 'warn').mockImplementation(() => logger);
      const error = jest.spyOn(logger, 'error').mockImplementation(() => logger);

      try {
        await request(app)
          .post('/api/unknown-endpoint')
          .set('Content-Type', 'application/json')
          .send(jsonOfExactSize(LIMIT_BYTES + 1));

        expect(warn).toHaveBeenCalledWith(
          'Request body rejected by parser',
          expect.objectContaining({
            type: 'entity.too.large',
            statusCode: 413,
            method: 'POST',
            path: '/api/unknown-endpoint',
          })
        );

        await request(app)
          .post('/api/unknown-endpoint')
          .set('Content-Type', 'application/json')
          .send('{ invalid }');

        expect(error).toHaveBeenCalledWith(
          'Request failed',
          expect.objectContaining({ category: 'invalid_json', statusCode: 400 })
        );
      } finally {
        warn.mockRestore();
        error.mockRestore();
      }
    });

    it('reports each failure exactly once so a retry cannot double-count it', async () => {
      const spies = spyOnLoggers();

      try {
        await request(app)
          .post('/api/unknown-endpoint')
          .set('Content-Type', 'application/json')
          .send(jsonOfExactSize(LIMIT_BYTES + 1));

        const all = spies.flatMap((spy) => spy.mock.calls);
        expect(all).toHaveLength(1);
        expect(all[0][0]).toBe('Request body rejected by parser');
      } finally {
        spies.forEach((spy) => spy.mockRestore());
      }
    });
  });

  describe('partial failure after the response has started', () => {
    const validBody = {
      userAddress: VALID_USER_ADDRESS,
      amount: VALID_AMOUNT,
      userSecret: 'S-submitted-signing-secret',
    };

    /**
     * Runs a handler that commits the response and then throws, recording every
     * chunk written to the socket so the test can prove nothing was appended
     * after the committed bytes.
     */
    const commitThenThrow = (mock: jest.Mock, message: string): string[] => {
      const written: string[] = [];

      mock.mockImplementationOnce((_req: any, res: any) => {
        const write = res.write.bind(res);
        const end = res.end.bind(res);
        res.write = (chunk: any, ...rest: unknown[]) => {
          written.push(String(chunk));
          return write(chunk, ...rest);
        };
        res.end = (chunk: any, ...rest: unknown[]) => {
          if (chunk) written.push(String(chunk));
          return end(chunk, ...rest);
        };

        res.writeHead(200, { 'Content-Type': 'text/plain' });
        res.write('partial-payload');
        throw new Error(message);
      });

      return written;
    };

    it('does not write a second response when a handler fails mid-stream', async () => {
      const written = commitThenThrow(
        mockWithdraw,
        'late failure after headers were flushed'
      );
      const error = jest.spyOn(logger, 'error').mockImplementation(() => logger);

      try {
        // The client may observe an aborted/incomplete response, because the
        // status line was already committed and the stream cannot be re-framed.
        await request(app)
          .post('/api/lending/withdraw')
          .send(validBody)
          .catch(() => undefined);

        // The invariant: nothing was appended after the committed chunk, so the
        // client can never receive a second status line or an error body spliced
        // onto the committed payload.
        expect(written.join('')).toBe('partial-payload');
        expect(written.join('')).not.toContain('Internal server error');

        // The failure is still observable exactly once, with a usable category.
        expect(error).toHaveBeenCalledTimes(1);
        expect(error).toHaveBeenCalledWith(
          'Request failed',
          expect.objectContaining({ category: 'server_error', statusCode: 500 })
        );
      } finally {
        error.mockRestore();
        mockWithdraw.mockReset();
      }
    });

    it('handles a non-Error throw after the response has started', async () => {
      // Express requires an Error for the delegated `next(err)`; a raw thrown
      // value must be normalized rather than crashing the handler.
      const written: string[] = [];

      mockWithdraw.mockImplementationOnce((_req: any, res: any) => {
        const write = res.write.bind(res);
        res.write = (chunk: any, ...rest: unknown[]) => {
          written.push(String(chunk));
          return write(chunk, ...rest);
        };

        res.writeHead(200, { 'Content-Type': 'text/plain' });
        res.write('partial-payload');
        throw 'a thrown string rather than an Error';
      });

      const error = jest.spyOn(logger, 'error').mockImplementation(() => logger);

      try {
        await request(app)
          .post('/api/lending/withdraw')
          .send(validBody)
          .catch(() => undefined);

        expect(written.join('')).toBe('partial-payload');
        expect(error).toHaveBeenCalledTimes(1);
      } finally {
        error.mockRestore();
        mockWithdraw.mockReset();
      }
    });

    it('keeps serving subsequent requests after a mid-stream failure', async () => {
      const written = commitThenThrow(mockRepay, 'late failure after headers were flushed');
      const error = jest.spyOn(logger, 'error').mockImplementation(() => logger);

      try {
        await request(app)
          .post('/api/lending/repay')
          .send(validBody)
          .catch(() => undefined);

        expect(written.join('')).toBe('partial-payload');

        const recovery = await request(app).post('/api/lending/deposit').send(validBody);

        expect(recovery.status).toBe(200);
        expect(recovery.body).toEqual({ success: true, transactionHash: 'tx-hash' });
      } finally {
        error.mockRestore();
        mockRepay.mockReset();
      }
    });
  });

  describe('determinism, retries, and concurrency', () => {
    it('produces byte-identical responses for repeated identical requests', async () => {
      const send = () =>
        request(app).post('/api/lending/deposit').send({ userAddress: 'bad-address' });

      const responses = await Promise.all([send(), send(), send(), send(), send()]);

      const shapes = responses.map((r) => `${r.status}:${JSON.stringify(r.body)}`);
      expect(new Set(shapes).size).toBe(1);
    });

    it('isolates concurrent valid, invalid, and boundary requests', async () => {
      const [valid, invalid, oversized, unknown, hook] = await Promise.all([
        request(app)
          .post('/api/lending/deposit')
          .send({ userAddress: VALID_USER_ADDRESS, amount: VALID_AMOUNT, userSecret: 'S-s' }),
        request(app).post('/api/lending/deposit').send({ userAddress: 'nope' }),
        request(app)
          .post('/api/unknown-endpoint')
          .set('Content-Type', 'application/json')
          .send(jsonOfExactSize(LIMIT_BYTES + 1)),
        request(app).get('/api/unknown'),
        request(app).post('/api/lending/hooks/indexer').send({ event: 'x' }),
      ]);

      expect(valid.status).toBe(200);
      expect(invalid.status).toBe(400);
      expect(oversized.status).toBe(413);
      expect(unknown.status).toBe(404);
      expect(hook.status).toBe(401);

      // Each request reached only its own handler exactly once.
      expect(mockDeposit).toHaveBeenCalledTimes(1);
      expect(mockProcessHook).not.toHaveBeenCalled();
    });

    it('does not let a rejected request mutate state observed by the next one', async () => {
      const invalid = await request(app)
        .post('/api/lending/deposit')
        .send({ userAddress: 'bad-address', amount: '-5', userSecret: '' });
      const valid = await request(app).post('/api/lending/deposit').send({
        userAddress: VALID_USER_ADDRESS,
        amount: VALID_AMOUNT,
        userSecret: 'S-s',
      });

      expect(invalid.status).toBe(400);
      expect(valid.status).toBe(200);
      // The controller received only the second, valid request's body.
      expect(mockDeposit).toHaveBeenCalledTimes(1);
      expect(mockDeposit.mock.calls[0][0].body).toEqual({
        userAddress: VALID_USER_ADDRESS,
        amount: VALID_AMOUNT,
        userSecret: 'S-s',
      });
    });

    it('rejects duplicate JSON keys deterministically rather than silently picking one', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .set('Content-Type', 'application/json')
        .send(
          `{"userAddress":"${VALID_USER_ADDRESS}","amount":"${VALID_AMOUNT}","userSecret":"S-a","userSecret":"S-b"}`
        );

      // `JSON.parse` keeps the last duplicate. What matters is that the same
      // bytes always produce the same outcome, and that only one value survives.
      expect(response.status).toBe(200);
      expect(mockDeposit).toHaveBeenCalledTimes(1);
      expect(mockDeposit.mock.calls[0][0].body.userSecret).toBe('S-b');

      const repeat = await request(app)
        .post('/api/lending/deposit')
        .set('Content-Type', 'application/json')
        .send(
          `{"userAddress":"${VALID_USER_ADDRESS}","amount":"${VALID_AMOUNT}","userSecret":"S-a","userSecret":"S-b"}`
        );

      expect(repeat.status).toBe(200);
      expect(mockDeposit.mock.calls[1][0].body.userSecret).toBe('S-b');
    });
  });
});

describe('app.ts rate limiting', () => {
  // A dedicated instance: the rate limit is read from the environment when the
  // module is first evaluated, so exercising the 429 path needs its own budget.
  let limitedApp: any;
  const LIMIT = 2;

  beforeAll(() => {
    process.env.RATE_LIMIT_MAX_REQUESTS = String(LIMIT);
    process.env.RATE_LIMIT_WINDOW_MS = '60000';
    jest.resetModules();
    limitedApp = require('../app').default;
  });

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('allows exactly the configured budget, then rejects with a JSON envelope', async () => {
    for (let i = 0; i < LIMIT; i++) {
      const allowed = await request(limitedApp).get('/api/health');
      expect(allowed.status).toBe(200);
    }

    const rejected = await request(limitedApp).get('/api/health');

    expect(rejected.status).toBe(429);
    expect(rejected.headers['content-type']).toMatch(/application\/json/);
    expect(rejected.body).toEqual({ success: false, error: RATE_LIMIT_MESSAGE });
    // The client needs the reset hint to back off deterministically.
    expect(rejected.headers['retry-after']).toBeDefined();
  });

  it('keeps rejecting deterministically once the budget is exhausted', async () => {
    const responses = await Promise.all([
      request(limitedApp).get('/api/health'),
      request(limitedApp).get('/api/health'),
      request(limitedApp).get('/api/health'),
    ]);

    for (const response of responses) {
      expect(response.status).toBe(429);
      expect(response.body).toEqual({ success: false, error: RATE_LIMIT_MESSAGE });
    }
  });

  it('does not rate-limit routes outside /api/', async () => {
    const responses = await Promise.all([
      request(limitedApp).get('/not-an-api-route'),
      request(limitedApp).get('/not-an-api-route'),
      request(limitedApp).get('/not-an-api-route'),
    ]);

    expect(responses.map((r) => r.status)).toEqual([404, 404, 404]);
  });

  describe('bucket key resolution', () => {
    it('prefers the resolved client address', () => {
      expect(rateLimitKey({ ip: '10.0.0.1', socket: { remoteAddress: '10.0.0.2' } })).toBe(
        '10.0.0.1'
      );
    });

    it('falls back to the socket address when no ip is resolved', () => {
      expect(rateLimitKey({ socket: { remoteAddress: '10.0.0.2' } })).toBe('10.0.0.2');
      expect(rateLimitKey({ ip: '', socket: { remoteAddress: '10.0.0.2' } })).toBe('10.0.0.2');
    });

    it('always yields a non-empty key so clients are never merged by accident', () => {
      for (const req of [{}, { ip: '' }, { socket: {} }, { ip: '', socket: {} }]) {
        const key = rateLimitKey(req);
        expect(key).toBe('unknown');
        expect(key.length).toBeGreaterThan(0);
      }
    });
  });
});
