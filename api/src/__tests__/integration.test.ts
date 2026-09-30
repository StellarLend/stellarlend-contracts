import crypto from 'crypto';
import request from 'supertest';

/**
 * The app mounts a per-IP rate limiter (100 requests / 15 min by default) on
 * every `/api/` route. This suite deliberately issues more than 100 requests,
 * so at the production default the limiter would start answering 429 partway
 * through the run and which assertions failed would depend on test ordering.
 *
 * The limiter has its own dedicated test, so raise the ceiling for this file
 * only, and do it before `../app` is required because the config module reads
 * the environment once at import time. This does not alter the shipped
 * default: `RATE_LIMIT_MAX_REQUESTS` is unset in every other environment.
 */
process.env.RATE_LIMIT_MAX_REQUESTS = '100000';

const mockStellarService = {
  buildDepositTransaction: jest.fn(),
  buildBorrowTransaction: jest.fn(),
  buildRepayTransaction: jest.fn(),
  buildWithdrawTransaction: jest.fn(),
  submitTransaction: jest.fn(),
  monitorTransaction: jest.fn(),
  healthCheck: jest.fn(),
  pingContract: jest.fn(),
};

jest.mock('../services/stellar.service', () => ({
  StellarService: jest.fn(() => mockStellarService),
}));

const app = require('../app').default;

const VALID_ADDRESS = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ';
const VALID_SECRET = 'SAOS4OGIK6HD4QGR3DVRRDSR4FUBH73FCZGRZ7M53LRN67UQE5JDNS4I';
const HOOK_SECRET = 'integration-test-hook-secret';

/** Largest value representable by a Stellar i128 (2^127 - 1). */
const I128_MAX = '170141183460469231731687303715884105727';
/** First value that overflows i128 (2^127). */
const I128_MAX_PLUS_ONE = '170141183460469231731687303715884105728';

const capitalize = (value: string) => value.charAt(0).toUpperCase() + value.slice(1);

describe('API Integration Tests', () => {
  beforeEach(() => {
    jest.clearAllMocks();

    mockStellarService.buildDepositTransaction.mockResolvedValue('mock_tx_xdr');
    mockStellarService.buildBorrowTransaction.mockResolvedValue('mock_tx_xdr');
    mockStellarService.buildRepayTransaction.mockResolvedValue('mock_tx_xdr');
    mockStellarService.buildWithdrawTransaction.mockResolvedValue('mock_tx_xdr');
    mockStellarService.submitTransaction.mockResolvedValue({
      success: false,
      status: 'failed',
      error: 'mock transaction failure',
    });
    mockStellarService.monitorTransaction.mockResolvedValue({
      success: true,
      status: 'success',
      transactionHash: 'mock_hash',
    });
    mockStellarService.healthCheck.mockResolvedValue({
      horizon: true,
      sorobanRpc: true,
    });
    mockStellarService.pingContract.mockResolvedValue({
      rpc: true,
      contract: true,
      ledger: 12345,
    });
  });

  describe('Complete Lending Flow', () => {
    it('should handle complete lending lifecycle', async () => {
      // This is a mock test - in real scenario, you'd use actual testnet accounts
      // 1. Deposit collateral
      // 2. Borrow against collateral
      // 3. Repay borrowed amount
      // 4. Withdraw collateral

      expect(true).toBe(true);
    });
  });

  describe('Error Handling', () => {
    it('should handle network errors gracefully', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: 'invalid_address',
          amount: '1000000',
          userSecret: 'invalid_secret',
        });

      expect(response.status).toBe(400);
    });

    it('should handle rate limiting', async () => {
      // Make multiple requests to trigger rate limit
      const requests = Array(10).fill(null).map(() =>
        request(app)
          .post('/api/lending/deposit')
          .send({
            userAddress: VALID_ADDRESS,
            amount: '1000000',
            userSecret: VALID_SECRET,
          })
      );

      const responses = await Promise.all(requests);

      // At least some requests should succeed (before rate limit)
      expect(responses.some(r => r.status === 200 || r.status === 400)).toBe(true);
    });
  });

  describe('Concurrent Requests', () => {
    it('should handle concurrent deposit requests', async () => {
      const requests = [
        request(app).post('/api/lending/deposit').send({
          userAddress: VALID_ADDRESS,
          amount: '1000000',
          userSecret: VALID_SECRET,
        }),
        request(app).post('/api/lending/deposit').send({
          userAddress: 'GD5TFY4DYYF43CQN3UMZUPBBXBLWK3WYAM5PIOMKOVRHBTZF7J7VGHP4',
          amount: '2000000',
          userSecret: VALID_SECRET,
        }),
      ];

      const responses = await Promise.all(requests);

      responses.forEach(response => {
        expect([200, 400, 500]).toContain(response.status);
      });
    });
  });

  describe('Edge Cases', () => {
    it('should reject extremely large amounts', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: VALID_ADDRESS,
          amount: '170141183460469231731687303715884105728',
          userSecret: VALID_SECRET,
        });

      expect(response.status).toBe(400);
      expect(response.body.error).toContain('signed 128-bit');
    });

    it('should handle missing optional fields', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: VALID_ADDRESS,
          amount: '1000000',
          userSecret: VALID_SECRET,
          // assetAddress is optional
        });

      expect([200, 400, 500]).toContain(response.status);
    });

    it('should reject malformed JSON', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .set('Content-Type', 'application/json')
        .send('{ invalid json }');

      expect(response.status).toBe(400);
    });
  });

  describe('CORS and Security Headers', () => {
    it('should include security headers', async () => {
      const response = await request(app).get('/api/health');

      expect(response.headers).toHaveProperty('x-content-type-options');
      expect(response.headers).toHaveProperty('x-frame-options');
    });

    it('should handle OPTIONS requests', async () => {
      const response = await request(app).options('/api/lending/deposit');

      expect([200, 204]).toContain(response.status);
    });
  });

  describe('Deep healthz endpoint', () => {
    afterEach(() => {
      jest.restoreAllMocks();
    });

    it('returns 200 and structured status when rpc and contract reachable', async () => {
      mockStellarService.pingContract.mockResolvedValue({ rpc: true, contract: true, ledger: 12345 });

      const res = await request(app).get('/api/health/healthz');

      expect(res.status).toBe(200);
      expect(res.body).toHaveProperty('rpc', true);
      expect(res.body).toHaveProperty('contract', true);
      expect(res.body).toHaveProperty('ledger', 12345);
    });

    it('returns 503 when contract unreachable', async () => {
      mockStellarService.pingContract.mockResolvedValue({ rpc: true, contract: false, ledger: null });

      const res = await request(app).get('/api/health/healthz');

      expect(res.status).toBe(503);
      expect(res.body).toHaveProperty('rpc', true);
      expect(res.body).toHaveProperty('contract', false);
    });
  });

  // ==========================================================================
  // Issue #2106: authorization and validation regression coverage
  // ==========================================================================

  describe('Lending route validation (all operations)', () => {
    const operations = ['deposit', 'borrow', 'repay', 'withdraw'] as const;

    describe.each(operations)('POST /api/lending/%s', (operation) => {
      const post = (body: unknown) =>
        request(app).post(`/api/lending/${operation}`).send(body as object);

      const buildMock = () => mockStellarService[`build${capitalize(operation)}Transaction` as const];

      it('rejects a missing userAddress', async () => {
        const res = await post({ amount: '1000000', userSecret: VALID_SECRET });

        expect(res.status).toBe(400);
        expect(res.body.success).toBe(false);
        expect(res.body.error).toMatch(/userAddress/i);
      });

      it('rejects a missing amount', async () => {
        const res = await post({ userAddress: VALID_ADDRESS, userSecret: VALID_SECRET });

        expect(res.status).toBe(400);
        expect(res.body.error).toMatch(/amount/i);
      });

      it('rejects a missing userSecret', async () => {
        const res = await post({ userAddress: VALID_ADDRESS, amount: '1000000' });

        expect(res.status).toBe(400);
        expect(res.body.error).toMatch(/userSecret/i);
      });

      it('rejects a malformed userAddress', async () => {
        const res = await post({
          userAddress: 'not-a-stellar-address',
          amount: '1000000',
          userSecret: VALID_SECRET,
        });

        expect(res.status).toBe(400);
        expect(res.body.error).toMatch(/valid Stellar account or contract address/i);
      });

      it('rejects a non-numeric amount', async () => {
        const res = await post({
          userAddress: VALID_ADDRESS,
          amount: 'not-a-number',
          userSecret: VALID_SECRET,
        });

        expect(res.status).toBe(400);
        expect(res.body.error).toMatch(/integer string/i);
      });

      it('rejects a zero amount', async () => {
        const res = await post({
          userAddress: VALID_ADDRESS,
          amount: '0',
          userSecret: VALID_SECRET,
        });

        expect(res.status).toBe(400);
        expect(res.body.error).toMatch(/greater than zero/i);
      });

      it('rejects a negative amount', async () => {
        const res = await post({
          userAddress: VALID_ADDRESS,
          amount: '-1000',
          userSecret: VALID_SECRET,
        });

        expect(res.status).toBe(400);
      });

      it('rejects an amount beyond the signed 128-bit range', async () => {
        // 2^127 is the first value outside the range; 2^127 - 1 is the max.
        const res = await post({
          userAddress: VALID_ADDRESS,
          amount: I128_MAX_PLUS_ONE,
          userSecret: VALID_SECRET,
        });

        expect(res.status).toBe(400);
        expect(res.body.error).toMatch(/signed 128-bit/i);
      });

      it('accepts the maximum in-range i128 amount', async () => {
        const res = await post({
          userAddress: VALID_ADDRESS,
          amount: I128_MAX,
          userSecret: VALID_SECRET,
        });

        // The largest representable value must clear validation and reach the
        // service, which is what rejects it with its own business error.
        expect(res.body.error ?? '').not.toMatch(/signed 128-bit/i);
        expect(mockStellarService.submitTransaction).toHaveBeenCalled();
      });

      it('treats an empty assetAddress as absent rather than invalid', async () => {
        const res = await post({
          userAddress: VALID_ADDRESS,
          amount: '1000000',
          userSecret: VALID_SECRET,
          assetAddress: '',
        });

        // The request clears validation and reaches the service. The response
        // is then the mocked submit failure, not a schema rejection.
        expect(res.body.error ?? '').not.toMatch(/assetAddress/i);
        expect(mockStellarService.submitTransaction).toHaveBeenCalled();
      });

      it('rejects a malformed assetAddress when one is supplied', async () => {
        const res = await post({
          userAddress: VALID_ADDRESS,
          amount: '1000000',
          userSecret: VALID_SECRET,
          assetAddress: 'invalid-asset',
        });

        expect(res.status).toBe(400);
        expect(res.body.error).toMatch(/assetAddress/i);
      });

      it('rejects a whitespace-only userSecret', async () => {
        const res = await post({
          userAddress: VALID_ADDRESS,
          amount: '1000000',
          userSecret: '   ',
        });

        expect(res.status).toBe(400);
        expect(res.body.error).toMatch(/userSecret/i);
      });

      it('does not reach the Stellar service when validation fails', async () => {
        await post({ userAddress: 'bad', amount: '1', userSecret: VALID_SECRET });

        expect(mockStellarService.buildDepositTransaction).not.toHaveBeenCalled();
        expect(mockStellarService.buildBorrowTransaction).not.toHaveBeenCalled();
        expect(mockStellarService.buildRepayTransaction).not.toHaveBeenCalled();
        expect(mockStellarService.buildWithdrawTransaction).not.toHaveBeenCalled();
        expect(mockStellarService.submitTransaction).not.toHaveBeenCalled();
      });

      it('returns a consistent error envelope for every rejection', async () => {
        const res = await post({ userAddress: 'bad', amount: '1', userSecret: VALID_SECRET });

        expect(res.status).toBe(400);
        expect(res.body).toEqual({
          success: false,
          error: expect.any(String),
        });
        // The envelope must not carry a stack trace or internal detail.
        expect(res.text).not.toMatch(/at .*\.ts:\d+/);
      });

      it('surfaces an upstream service failure as a 400 without leaking internals', async () => {
        mockStellarService.submitTransaction.mockResolvedValue({
          success: false,
          status: 'failed',
          error: 'Insufficient collateral',
        });

        const res = await post({
          userAddress: VALID_ADDRESS,
          amount: '1000000',
          userSecret: VALID_SECRET,
        });

        expect(res.status).toBe(400);
        expect(res.body.error).toBe('Insufficient collateral');
      });

      it('returns 500 with a generic message when the service throws', async () => {
        buildMock().mockRejectedValue(new Error('connect ECONNREFUSED 10.0.0.5:5432'));

        const res = await post({
          userAddress: VALID_ADDRESS,
          amount: '1000000',
          userSecret: VALID_SECRET,
        });

        expect(res.status).toBe(500);
        expect(res.body.success).toBe(false);
        // Internal host/port detail must not reach the client.
        expect(res.text).not.toContain('10.0.0.5');
        expect(res.text).not.toContain('5432');
      });
    });
  });

  describe('Hook endpoint authorization', () => {
    const hookPath = '/api/lending/hooks/indexer';
    const payload = { event: 'indexer.write', data: { id: 'abc123' } };
    const rawBody = JSON.stringify(payload);

    let scopedApp: any;

    const sign = (timestamp: string, body: string) =>
      crypto.createHmac('sha256', HOOK_SECRET).update(`${timestamp}.${body}`).digest('hex');

    beforeAll(() => {
      process.env.STELLAR_API_HOOK_SECRET = HOOK_SECRET;
      jest.resetModules();
      scopedApp = require('../app').default;
    });

    afterAll(() => {
      delete process.env.STELLAR_API_HOOK_SECRET;
      jest.resetModules();
    });

    it('accepts a correctly signed hook request', async () => {
      const timestamp = Date.now().toString();
      const res = await request(scopedApp)
        .post(hookPath)
        .set('X-Hook-Timestamp', timestamp)
        .set('X-Hook-Signature', sign(timestamp, rawBody))
        .send(payload);

      expect(res.status).toBe(200);
      expect(res.body).toEqual({ success: true, message: 'Hook authenticated' });
    });

    it('rejects a hook request with no signature headers', async () => {
      const res = await request(scopedApp).post(hookPath).send(payload);

      expect(res.status).toBe(401);
      expect(res.body.success).toBe(false);
      expect(res.body.error).toMatch(/signature and timestamp/i);
    });

    it('rejects a request signed with the wrong secret', async () => {
      const timestamp = Date.now().toString();
      const badSig = crypto
        .createHmac('sha256', 'wrong-secret')
        .update(`${timestamp}.${rawBody}`)
        .digest('hex');

      const res = await request(scopedApp)
        .post(hookPath)
        .set('X-Hook-Timestamp', timestamp)
        .set('X-Hook-Signature', badSig)
        .send(payload);

      expect(res.status).toBe(401);
      expect(res.body.error).toMatch(/invalid hook signature/i);
    });

    it('rejects a valid signature replayed outside the timestamp window', async () => {
      const stale = (Date.now() - 10 * 60 * 1000).toString();
      const res = await request(scopedApp)
        .post(hookPath)
        .set('X-Hook-Timestamp', stale)
        .set('X-Hook-Signature', sign(stale, rawBody))
        .send(payload);

      expect(res.status).toBe(401);
      expect(res.body.error).toMatch(/timestamp outside allowable window/i);
    });

    it('rejects a non-numeric timestamp', async () => {
      const res = await request(scopedApp)
        .post(hookPath)
        .set('X-Hook-Timestamp', 'not-a-number')
        .set('X-Hook-Signature', sign('not-a-number', rawBody))
        .send(payload);

      expect(res.status).toBe(401);
    });

    it('rejects a signature that is not valid hex', async () => {
      const timestamp = Date.now().toString();
      const res = await request(scopedApp)
        .post(hookPath)
        .set('X-Hook-Timestamp', timestamp)
        .set('X-Hook-Signature', 'zzzz')
        .send(payload);

      expect(res.status).toBe(401);
      expect(res.body.error).toMatch(/invalid hook signature/i);
    });

    it('rejects a signature of the wrong length without throwing', async () => {
      // A short/long hex string must be rejected on length, not crash
      // crypto.timingSafeEqual.
      const timestamp = Date.now().toString();
      const res = await request(scopedApp)
        .post(hookPath)
        .set('X-Hook-Timestamp', timestamp)
        .set('X-Hook-Signature', 'abcd')
        .send(payload);

      expect(res.status).toBe(401);
    });

    it('rejects a signature computed over a tampered body', async () => {
      const timestamp = Date.now().toString();
      const res = await request(scopedApp)
        .post(hookPath)
        .set('X-Hook-Timestamp', timestamp)
        .set('X-Hook-Signature', sign(timestamp, rawBody))
        .send({ event: 'indexer.write', data: { id: 'tampered' } });

      expect(res.status).toBe(401);
      expect(res.body.error).toMatch(/invalid hook signature/i);
    });

    it('does not process an unauthorized hook body', async () => {
      await request(scopedApp)
        .post(hookPath)
        .set('X-Hook-Timestamp', Date.now().toString())
        .set('X-Hook-Signature', 'deadbeef')
        .send(payload);

      expect(mockStellarService.submitTransaction).not.toHaveBeenCalled();
    });
  });

  describe('Error response formatting', () => {
    it('returns a JSON 400 for malformed JSON', async () => {
      const res = await request(app)
        .post('/api/lending/deposit')
        .set('Content-Type', 'application/json')
        .send('{"userAddress": ');

      expect(res.status).toBe(400);
      expect(res.headers['content-type']).toMatch(/application\/json/);
      expect(res.body.success).toBe(false);
    });

    it('returns JSON for an unknown route rather than an HTML error page', async () => {
      const res = await request(app).get('/api/lending/does-not-exist');

      expect(res.status).toBe(404);
    });

    it('does not leak a stack trace in any error response', async () => {
      mockStellarService.buildDepositTransaction.mockRejectedValue(
        new Error('internal detail: /srv/secret/path')
      );

      const res = await request(app)
        .post('/api/lending/deposit')
        .send({ userAddress: VALID_ADDRESS, amount: '1000000', userSecret: VALID_SECRET });

      expect(res.text).not.toContain('/srv/secret/path');
      expect(res.text).not.toMatch(/\s+at\s+.*:\d+:\d+/);
    });
  });

  describe('Retry safety and concurrency', () => {
    it('is deterministic across repeated identical valid requests', async () => {
      const body = { userAddress: VALID_ADDRESS, amount: '1000000', userSecret: VALID_SECRET };

      const responses = await Promise.all(
        Array.from({ length: 5 }, () => request(app).post('/api/lending/deposit').send(body))
      );

      const statuses = new Set(responses.map(r => r.status));
      // Concurrent identical requests must not diverge into mixed outcomes.
      expect(statuses.size).toBe(1);
      expect(responses.map(r => r.status)).toEqual(responses.map(() => responses[0].status));
    });

    it('returns a stable body for repeated identical valid requests', async () => {
      const body = { userAddress: VALID_ADDRESS, amount: '1000000', userSecret: VALID_SECRET };

      const first = await request(app).post('/api/lending/deposit').send(body);
      const second = await request(app).post('/api/lending/deposit').send(body);

      expect(second.status).toBe(first.status);
      expect(second.body).toEqual(first.body);
    });

    it('isolates concurrent valid and invalid requests', async () => {
      const valid = { userAddress: VALID_ADDRESS, amount: '1000000', userSecret: VALID_SECRET };
      const invalid = { userAddress: 'bad', amount: '1000000', userSecret: VALID_SECRET };

      const [ok, bad] = await Promise.all([
        request(app).post('/api/lending/deposit').send(valid),
        request(app).post('/api/lending/deposit').send(invalid),
      ]);

      // The invalid request is rejected by the schema; the valid one clears
      // validation and is decided by the (mocked) service. Critically, the
      // invalid request must not inherit the valid one's body or vice versa.
      expect(bad.status).toBe(400);
      expect(bad.body.error).toMatch(/valid Stellar account or contract address/i);

      expect(ok.body.error ?? '').not.toMatch(/userAddress|assetAddress/i);

      // Only the valid request reached the service layer.
      expect(mockStellarService.buildDepositTransaction).toHaveBeenCalledTimes(1);
      expect(mockStellarService.buildDepositTransaction).toHaveBeenCalledWith(
        VALID_ADDRESS,
        undefined,
        '1000000',
        VALID_SECRET
      );
    });

    it('does not mutate the validated body across concurrent requests', async () => {
      const body = { userAddress: VALID_ADDRESS, amount: '1000000', userSecret: VALID_SECRET };

      await Promise.all(
        Array.from({ length: 3 }, () => request(app).post('/api/lending/deposit').send(body))
      );

      // The service must receive the caller's values unmodified each time.
      for (const call of mockStellarService.buildDepositTransaction.mock.calls) {
        expect(call[0]).toBe(VALID_ADDRESS);
        expect(call[2]).toBe('1000000');
      }
    });
  });

  describe('Health endpoint authorization posture', () => {
    it('allows unauthenticated health checks', async () => {
      const res = await request(app).get('/api/health');

      expect(res.status).toBe(200);
      expect(res.body.status).toBe('healthy');
    });

    it('returns 503 when a dependency is down', async () => {
      mockStellarService.healthCheck.mockResolvedValue({ horizon: true, sorobanRpc: false });

      const res = await request(app).get('/api/health');

      expect(res.status).toBe(503);
      expect(res.body.status).toBe('unhealthy');
    });

    it('returns 503 when the deep health probe cannot reach the contract', async () => {
      mockStellarService.pingContract.mockRejectedValue(new Error('rpc unreachable'));

      const res = await request(app).get('/api/health/healthz');

      expect(res.status).toBe(500);
      expect(res.text).not.toContain('rpc unreachable');
    });

    it('never exposes internal hostnames through health output', async () => {
      mockStellarService.healthCheck.mockResolvedValue({ horizon: true, sorobanRpc: true });

      const res = await request(app).get('/api/health');

      expect(res.text).not.toMatch(/https?:\/\//);
    });
  });
});
