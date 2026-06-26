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
