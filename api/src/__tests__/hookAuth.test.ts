import crypto from 'crypto';
import request from 'supertest';

let app: any;

const HOOK_SECRET = 'test-hook-secret-rotation-ready';

beforeAll(() => {
  process.env.STELLAR_API_HOOK_SECRET = HOOK_SECRET;
  jest.resetModules();
  app = require('../app').default;
});

describe('Hook HMAC middleware', () => {
  const hookPath = '/api/lending/hooks/indexer';
  const payload = { event: 'indexer.write', data: { id: 'abc123' } };
  const rawBody = JSON.stringify(payload);

  const sign = (timestamp: string, body: string) =>
    crypto
      .createHmac('sha256', HOOK_SECRET)
      .update(`${timestamp}.${body}`)
      .digest('hex');

  it('accepts valid hook requests with matching signature and timestamp', async () => {
    const timestamp = Date.now().toString();
    const response = await request(app)
      .post(hookPath)
      .set('X-Hook-Timestamp', timestamp)
      .set('X-Hook-Signature', sign(timestamp, rawBody))
      .send(payload);

    expect(response.status).toBe(200);
    expect(response.body).toEqual({ success: true, message: 'Hook authenticated' });
  });

  it('rejects hook requests with missing headers', async () => {
    const response = await request(app).post(hookPath).send(payload);

    expect(response.status).toBe(401);
    expect(response.body.success).toBe(false);
    expect(response.body.error).toMatch(/signature and timestamp/i);
  });

  it('rejects hook requests with invalid signature', async () => {
    const timestamp = Date.now().toString();
    const response = await request(app)
      .post(hookPath)
      .set('X-Hook-Timestamp', timestamp)
      .set('X-Hook-Signature', 'invalidsignature')
      .send(payload);

    expect(response.status).toBe(401);
    expect(response.body.error).toMatch(/invalid hook signature/i);
  });

  it('rejects hook requests outside the 5-minute timestamp window', async () => {
    const timestamp = (Date.now() - 10 * 60 * 1000).toString();
    const response = await request(app)
      .post(hookPath)
      .set('X-Hook-Timestamp', timestamp)
      .set('X-Hook-Signature', sign(timestamp, rawBody))
      .send(payload);

    expect(response.status).toBe(401);
    expect(response.body.error).toMatch(/timestamp outside allowable window/i);
  });
});

describe('Hook HMAC header handling', () => {
  const payload = { event: 'indexer.write', data: { id: 'abc123' } };
  const rawBody = JSON.stringify(payload);
  const sign = (timestamp: string, body: string) =>
    crypto.createHmac('sha256', HOOK_SECRET).update(`${timestamp}.${body}`).digest('hex');

  const hookReq = (headers: Record<string, unknown>, extra: Record<string, unknown> = {}) =>
    ({ headers, body: payload, ...extra }) as any;

  // Required lazily: `config` snapshots the environment at import time, and the
  // hook secret is only set in `beforeAll`.
  const hookHmac = () => require('../middleware/auth').verifyHookHmac;

  it('takes the first value when the signature header repeats', () => {
    const timestamp = Date.now().toString();
    const next = jest.fn();

    hookHmac()(
      hookReq(
        {
          'x-hook-signature': [sign(timestamp, rawBody), 'second'],
          'x-hook-timestamp': [timestamp, '1'],
        },
        { rawBody }
      ),
      {} as any,
      next
    );

    expect(next).toHaveBeenCalled();
  });

  it('rejects when the repeated timestamp header is not numeric', () => {
    const next = jest.fn();

    expect(() =>
      hookHmac()(
        hookReq({ 'x-hook-signature': 'abcd', 'x-hook-timestamp': ['not-a-number'] }, { rawBody }),
        {} as any,
        next
      )
    ).toThrow('Invalid hook timestamp');
  });

  it('rejects a signature of a different length before comparing bytes', () => {
    const timestamp = Date.now().toString();
    const next = jest.fn();

    expect(() =>
      hookHmac()(
        hookReq({ 'x-hook-signature': 'abcd', 'x-hook-timestamp': timestamp }, { rawBody }),
        {} as any,
        next
      )
    ).toThrow('Invalid hook signature');
  });

  it('falls back to the JSON-encoded body when no raw body was captured', () => {
    const timestamp = Date.now().toString();
    const next = jest.fn();

    hookHmac()(
      hookReq(
        { 'x-hook-signature': sign(timestamp, rawBody), 'x-hook-timestamp': timestamp },
        { rawBody: undefined }
      ),
      {} as any,
      next
    );

    expect(next).toHaveBeenCalled();
  });

  it('rejects a signature computed over a different raw body', () => {
    const timestamp = Date.now().toString();
    const next = jest.fn();

    expect(() =>
      hookHmac()(
        hookReq(
          { 'x-hook-signature': sign(timestamp, rawBody), 'x-hook-timestamp': timestamp },
          { rawBody: '{"tampered":true}' }
        ),
        {} as any,
        next
      )
    ).toThrow('Invalid hook signature');
  });

  it('rejects every request when the hook secret is not configured', () => {
    const timestamp = Date.now().toString();
    const next = jest.fn();
    const original = process.env.STELLAR_API_HOOK_SECRET;
    delete process.env.STELLAR_API_HOOK_SECRET;
    jest.resetModules();
    const unconfigured = hookHmac();

    try {
      expect(() =>
        unconfigured(
          hookReq(
            { 'x-hook-signature': sign(timestamp, rawBody), 'x-hook-timestamp': timestamp },
            { rawBody }
          ),
          {} as any,
          next
        )
      ).toThrow('Hook authentication secret is not configured');
    } finally {
      process.env.STELLAR_API_HOOK_SECRET = original;
      jest.resetModules();
    }
  });
});
