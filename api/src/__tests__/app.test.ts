import crypto from 'crypto';
import request from 'supertest';

const mockProcessHook = jest.fn((_request: unknown, response: any) =>
  response.status(200).json({ success: true, message: 'Hook authenticated' })
);
const mockDeposit = jest.fn();

jest.mock('../controllers/lending.controller', () => ({
  processHook: mockProcessHook,
  deposit: mockDeposit,
  borrow: jest.fn(),
  repay: jest.fn(),
  withdraw: jest.fn(),
  getActivity: jest.fn(),
  healthCheck: jest.fn(),
  deepHealthCheck: jest.fn(),
}));

const HOOK_SECRET = 'app-regression-hook-secret';
process.env.STELLAR_API_HOOK_SECRET = HOOK_SECRET;

jest.resetModules();
const app = require('../app').default;

describe('API app authorization and validation', () => {
  const hookPath = '/api/lending/hooks/indexer';
  const timestamp = Date.now().toString();
  const rawBody = '{ "event": "indexer.write", "data": { "id": "abc123" } }';
  const sign = (body: string) =>
    crypto
      .createHmac('sha256', HOOK_SECRET)
      .update(`${timestamp}.${body}`)
      .digest('hex');

  beforeEach(() => {
    jest.clearAllMocks();
  });

  it('authenticates hooks against the exact request body bytes', async () => {
    const response = await request(app)
      .post(hookPath)
      .set('Content-Type', 'application/json')
      .set('X-Hook-Timestamp', timestamp)
      .set('X-Hook-Signature', sign(rawBody))
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
      .set('X-Hook-Signature', sign(JSON.stringify(JSON.parse(rawBody))))
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
        amount: '1000000',
        userSecret: 'submitted-secret',
      });

    expect(response.status).toBe(400);
    expect(response.body.success).toBe(false);
    expect(response.body.error).toContain('valid Stellar');
    expect(JSON.stringify(response.body)).not.toContain('submitted-secret');
    expect(mockDeposit).not.toHaveBeenCalled();
  });

  it('rejects an oversized JSON body with 413 before invoking the controller', async () => {
    const response = await request(app)
      .post('/api/lending/deposit')
      .send({
        userAddress: 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK',
        amount: '1000000',
        userSecret: 'x'.repeat(200 * 1024),
      });

    expect(response.status).toBe(413);
    expect(response.body).toEqual({ error: 'Payload too large' });
    expect(mockDeposit).not.toHaveBeenCalled();
  });

  it('keeps independent per-client counters when TRUST_PROXY=1', async () => {
    process.env.TRUST_PROXY = '1';
    process.env.RATE_LIMIT_MAX_REQUESTS = '2';

    jest.resetModules();
    const proxiedApp = require('../app').default;

    try {
      const clientA = '198.51.100.10';
      const clientB = '203.0.113.20';

      await request(proxiedApp).get('/api/rate-limit-test').set('X-Forwarded-For', clientA);
      await request(proxiedApp).get('/api/rate-limit-test').set('X-Forwarded-For', clientA);

      const exhaustedA = await request(proxiedApp)
        .get('/api/rate-limit-test')
        .set('X-Forwarded-For', clientA);
      expect(exhaustedA.status).toBe(429);

      const allowedB = await request(proxiedApp)
        .get('/api/rate-limit-test')
        .set('X-Forwarded-For', clientB);
      expect(allowedB.status).not.toBe(429);
    } finally {
      delete process.env.TRUST_PROXY;
      delete process.env.RATE_LIMIT_MAX_REQUESTS;
    }
  });
});
