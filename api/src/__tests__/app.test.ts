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
});
