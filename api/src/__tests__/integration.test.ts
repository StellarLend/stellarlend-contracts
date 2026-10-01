import request from 'supertest';

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

const VALID_USER_ADDRESS = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ';
const VALID_ASSET_ADDRESS = 'GD5TFY4DYYF43CQN3UMZUPBBXBLWK3WYAM5PIOMKOVRHBTZF7J7VGHP4';
const VALID_USER_SECRET = 'SAOS4OGIK6HD4QGR3DVRRDSR4FUBH73FCZGRZ7M53LRN67UQE5JDNS4I';
const I128_MAX = '170141183460469231731687303715884105727';
const I128_OVERFLOW = '170141183460469231731687303715884105728';

const validRequest = (amount = '1000000') => ({
  userAddress: VALID_USER_ADDRESS,
  assetAddress: VALID_ASSET_ADDRESS,
  amount,
  userSecret: VALID_USER_SECRET,
});

const successfulMonitorResult = {
  success: true,
  status: 'success',
  transactionHash: 'mock_hash',
};

describe('API Integration Tests', () => {
  beforeEach(() => {
    for (const fn of Object.values(mockStellarService)) {
      fn.mockReset();
    }

    mockStellarService.buildDepositTransaction.mockResolvedValue('mock_tx_xdr');
    mockStellarService.buildBorrowTransaction.mockResolvedValue('mock_tx_xdr');
    mockStellarService.buildRepayTransaction.mockResolvedValue('mock_tx_xdr');
    mockStellarService.buildWithdrawTransaction.mockResolvedValue('mock_tx_xdr');
    mockStellarService.submitTransaction.mockResolvedValue({
      success: true,
      status: 'success',
      transactionHash: 'mock_hash',
    });
    mockStellarService.monitorTransaction.mockResolvedValue(successfulMonitorResult);
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

  describe('Successful lending paths', () => {
    it.each([
      ['deposit', 'buildDepositTransaction'],
      ['borrow', 'buildBorrowTransaction'],
      ['repay', 'buildRepayTransaction'],
      ['withdraw', 'buildWithdrawTransaction'],
    ] as const)('processes a valid %s request through build, submit, and monitor', async (route, builderName) => {
      const response = await request(app)
        .post(`/api/lending/${route}`)
        .send(validRequest());

      expect(response.status).toBe(200);
      expect(response.body).toEqual(successfulMonitorResult);
      expect(mockStellarService[builderName]).toHaveBeenCalledWith(
        VALID_USER_ADDRESS,
        VALID_ASSET_ADDRESS,
        '1000000',
        VALID_USER_SECRET
      );
      expect(mockStellarService.submitTransaction).toHaveBeenCalledWith('mock_tx_xdr');
      expect(mockStellarService.monitorTransaction).toHaveBeenCalledWith('mock_hash');
    });
  });

  describe('Failure and recovery paths', () => {
    it('returns the submission failure and does not monitor a rejected transaction', async () => {
      const submissionFailure = {
        success: false,
        status: 'failed',
        error: 'transaction rejected',
      };
      mockStellarService.submitTransaction.mockResolvedValue(submissionFailure);

      const response = await request(app)
        .post('/api/lending/deposit')
        .send(validRequest());

      expect(response.status).toBe(400);
      expect(response.body).toEqual(submissionFailure);
      expect(mockStellarService.monitorTransaction).not.toHaveBeenCalled();
    });

    it('maps a service exception to a generic 500 response', async () => {
      mockStellarService.buildDepositTransaction.mockRejectedValue(new Error('RPC unavailable'));

      const response = await request(app)
        .post('/api/lending/deposit')
        .send(validRequest());

      expect(response.status).toBe(500);
      expect(response.body).toEqual({
        success: false,
        error: 'Internal server error',
      });
      expect(mockStellarService.submitTransaction).not.toHaveBeenCalled();
      expect(mockStellarService.monitorTransaction).not.toHaveBeenCalled();
    });

    it('recovers cleanly when the caller retries after a transient build failure', async () => {
      mockStellarService.buildDepositTransaction
        .mockRejectedValueOnce(new Error('temporary RPC failure'))
        .mockResolvedValueOnce('retry_tx_xdr');

      const first = await request(app)
        .post('/api/lending/deposit')
        .send(validRequest());
      const second = await request(app)
        .post('/api/lending/deposit')
        .send(validRequest());

      expect(first.status).toBe(500);
      expect(first.body.error).toBe('Internal server error');
      expect(second.status).toBe(200);
      expect(second.body).toEqual(successfulMonitorResult);
      expect(mockStellarService.buildDepositTransaction).toHaveBeenCalledTimes(2);
      expect(mockStellarService.submitTransaction).toHaveBeenCalledTimes(1);
      expect(mockStellarService.submitTransaction).toHaveBeenCalledWith('retry_tx_xdr');
      expect(mockStellarService.monitorTransaction).toHaveBeenCalledTimes(1);
    });

    it('returns a generic 500 when the deep health dependency throws', async () => {
      mockStellarService.pingContract.mockRejectedValue(new Error('private RPC details'));

      const response = await request(app).get('/api/health/healthz');

      expect(response.status).toBe(500);
      expect(response.body).toEqual({
        success: false,
        error: 'Internal server error',
      });
      expect(JSON.stringify(response.body)).not.toContain('private RPC details');
    });
  });

  describe('Validation boundaries', () => {
    it('rejects malformed addresses before any transaction is built', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          ...validRequest(),
          userAddress: 'invalid_address',
        });

      expect(response.status).toBe(400);
      expect(response.body.success).toBe(false);
      expect(response.body.error).toContain('valid Stellar');
      expect(mockStellarService.buildDepositTransaction).not.toHaveBeenCalled();
    });

    it.each(['0', '-1', '1.5', 'not-a-number'])('rejects invalid amount %s before controller execution', async amount => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send(validRequest(amount));

      expect(response.status).toBe(400);
      expect(mockStellarService.buildDepositTransaction).not.toHaveBeenCalled();
    });

    it('accepts the maximum signed i128 amount', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send(validRequest(I128_MAX));

      expect(response.status).toBe(200);
      expect(mockStellarService.buildDepositTransaction).toHaveBeenCalledWith(
        VALID_USER_ADDRESS,
        VALID_ASSET_ADDRESS,
        I128_MAX,
        VALID_USER_SECRET
      );
    });

    it('rejects an amount one unit above signed i128 max before controller execution', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send(validRequest(I128_OVERFLOW));

      expect(response.status).toBe(400);
      expect(response.body.error).toContain('signed 128-bit');
      expect(mockStellarService.buildDepositTransaction).not.toHaveBeenCalled();
    });

    it('normalizes an omitted optional asset address to undefined', async () => {
      const { assetAddress: _assetAddress, ...body } = validRequest();

      const response = await request(app)
        .post('/api/lending/deposit')
        .send(body);

      expect(response.status).toBe(200);
      expect(mockStellarService.buildDepositTransaction).toHaveBeenCalledWith(
        VALID_USER_ADDRESS,
        undefined,
        '1000000',
        VALID_USER_SECRET
      );
    });

    it('normalizes an empty optional asset address to undefined', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          ...validRequest(),
          assetAddress: '',
        });

      expect(response.status).toBe(200);
      expect(mockStellarService.buildDepositTransaction).toHaveBeenCalledWith(
        VALID_USER_ADDRESS,
        undefined,
        '1000000',
        VALID_USER_SECRET
      );
    });

    it('rejects a missing user secret before controller execution', async () => {
      const { userSecret: _userSecret, ...body } = validRequest();

      const response = await request(app)
        .post('/api/lending/deposit')
        .send(body);

      expect(response.status).toBe(400);
      expect(mockStellarService.buildDepositTransaction).not.toHaveBeenCalled();
    });

    it('rejects malformed JSON without executing transaction code', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .set('Content-Type', 'application/json')
        .send('{ invalid json }');

      expect(response.status).toBe(400);
      expect(response.body.success).toBe(false);
      expect(mockStellarService.buildDepositTransaction).not.toHaveBeenCalled();
    });
  });

  describe('Duplicate and concurrent requests', () => {
    it('handles duplicate valid requests as isolated invocations', async () => {
      const [first, second] = await Promise.all([
        request(app).post('/api/lending/deposit').send(validRequest()),
        request(app).post('/api/lending/deposit').send(validRequest()),
      ]);

      expect(first.status).toBe(200);
      expect(second.status).toBe(200);
      expect(first.body).toEqual(successfulMonitorResult);
      expect(second.body).toEqual(successfulMonitorResult);
      expect(mockStellarService.buildDepositTransaction).toHaveBeenCalledTimes(2);
      expect(mockStellarService.submitTransaction).toHaveBeenCalledTimes(2);
      expect(mockStellarService.monitorTransaction).toHaveBeenCalledTimes(2);
    });

    it('keeps concurrent request results isolated when they complete independently', async () => {
      mockStellarService.buildDepositTransaction.mockImplementation(
        async (_userAddress: string, _assetAddress: string | undefined, amount: string) => `xdr:${amount}`
      );
      mockStellarService.submitTransaction.mockImplementation(async (txXdr: string) => ({
        success: true,
        status: 'success',
        transactionHash: `hash:${txXdr}`,
      }));
      mockStellarService.monitorTransaction.mockImplementation(async (transactionHash: string) => ({
        success: true,
        status: 'success',
        transactionHash,
      }));

      const [first, second] = await Promise.all([
        request(app).post('/api/lending/deposit').send(validRequest('1000000')),
        request(app).post('/api/lending/deposit').send(validRequest('2000000')),
      ]);

      expect(first.status).toBe(200);
      expect(second.status).toBe(200);
      expect(first.body.transactionHash).toBe('hash:xdr:1000000');
      expect(second.body.transactionHash).toBe('hash:xdr:2000000');
      expect(mockStellarService.monitorTransaction).toHaveBeenCalledWith('hash:xdr:1000000');
      expect(mockStellarService.monitorTransaction).toHaveBeenCalledWith('hash:xdr:2000000');
    });
  });

  describe('Health and HTTP boundaries', () => {
    it('returns healthy status when all shallow health dependencies are reachable', async () => {
      const response = await request(app).get('/api/health');

      expect(response.status).toBe(200);
      expect(response.body.status).toBe('healthy');
      expect(response.body.services).toEqual({ horizon: true, sorobanRpc: true });
    });

    it('returns 503 when a shallow health dependency is unavailable', async () => {
      mockStellarService.healthCheck.mockResolvedValue({
        horizon: true,
        sorobanRpc: false,
      });

      const response = await request(app).get('/api/health');

      expect(response.status).toBe(503);
      expect(response.body.status).toBe('unhealthy');
    });

    it('returns structured deep health status when rpc and contract are reachable', async () => {
      const response = await request(app).get('/api/health/healthz');

      expect(response.status).toBe(200);
      expect(response.body).toEqual(expect.objectContaining({
        rpc: true,
        contract: true,
        ledger: 12345,
      }));
    });

    it('returns 503 when the contract is unreachable', async () => {
      mockStellarService.pingContract.mockResolvedValue({
        rpc: true,
        contract: false,
        ledger: null,
      });

      const response = await request(app).get('/api/health/healthz');

      expect(response.status).toBe(503);
      expect(response.body).toEqual(expect.objectContaining({
        rpc: true,
        contract: false,
        ledger: null,
      }));
    });

    it('includes security headers', async () => {
      const response = await request(app).get('/api/health');

      expect(response.headers).toHaveProperty('x-content-type-options');
      expect(response.headers).toHaveProperty('x-frame-options');
    });

    it('handles CORS preflight without invoking lending services', async () => {
      const response = await request(app)
        .options('/api/lending/deposit')
        .set('Origin', 'https://example.com')
        .set('Access-Control-Request-Method', 'POST');

      expect([200, 204]).toContain(response.status);
      expect(response.headers).toHaveProperty('access-control-allow-origin');
      expect(mockStellarService.buildDepositTransaction).not.toHaveBeenCalled();
    });
  });
});
