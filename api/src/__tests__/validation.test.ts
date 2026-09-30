import request from 'supertest';
import { z } from 'zod';
import { validateBody } from '../middleware/validation';
import { I128String, StellarAddress } from '../utils/validators';

const mockStellarService = {
  buildDepositTransaction: jest.fn(),
  submitTransaction: jest.fn(),
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

describe('Validation Middleware', () => {
  beforeEach(() => {
    jest.clearAllMocks();

    mockStellarService.buildDepositTransaction = jest.fn().mockResolvedValue('mock_tx_xdr');
    mockStellarService.submitTransaction = jest.fn().mockResolvedValue({
      success: false,
      status: 'failed',
      error: 'mock transaction failure',
    });
  });

  describe('Shared Validators', () => {
    it('should accept valid Stellar addresses', () => {
      expect(StellarAddress.safeParse(VALID_USER_ADDRESS).success).toBe(true);
    });

    it('should reject malformed Stellar addresses', () => {
      expect(StellarAddress.safeParse('GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX').success).toBe(false);
      expect(StellarAddress.safeParse('invalid_address').success).toBe(false);
    });

    it('should accept signed i128 integer strings', () => {
      expect(I128String.safeParse(I128_MAX).success).toBe(true);
      expect(I128String.safeParse('-170141183460469231731687303715884105728').success).toBe(true);
    });

    it('should reject non-integer and out-of-range i128 strings', () => {
      expect(I128String.safeParse('1.5').success).toBe(false);
      expect(I128String.safeParse('abc').success).toBe(false);
      expect(I128String.safeParse(I128_OVERFLOW).success).toBe(false);
    });

    it('should pass non-zod validator errors to next middleware', () => {
      const error = new Error('custom parser failure');
      const schema = {
        parse: jest.fn(() => {
          throw error;
        }),
      } as unknown as z.ZodSchema;
      const request = { body: { userAddress: VALID_USER_ADDRESS } } as any;
      const next = jest.fn();

      validateBody(schema)(request, {} as any, next);

      expect(next).toHaveBeenCalledWith(error);
    });
  });

  describe('Deposit Validation', () => {
    it('should reject empty userAddress', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          amount: '1000000',
          userSecret: VALID_USER_SECRET,
        });

      expect(response.status).toBe(400);
      expect(response.body.success).toBe(false);
      expect(response.body.error).toContain('userAddress');
    });

    it('should reject malformed userAddress before controller execution', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: 'invalid_address',
          amount: '1000000',
          userSecret: VALID_USER_SECRET,
        });

      expect(response.status).toBe(400);
      expect(response.body.success).toBe(false);
      expect(response.body.error).toContain('valid Stellar');
      expect(mockStellarService.buildDepositTransaction).not.toHaveBeenCalled();
    });

    it('should reject zero amount', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: VALID_USER_ADDRESS,
          amount: '0',
          userSecret: VALID_USER_SECRET,
        });

      expect(response.status).toBe(400);
      expect(response.body.error).toContain('greater than zero');
    });

    it('should reject negative amount', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: VALID_USER_ADDRESS,
          amount: '-1000',
          userSecret: VALID_USER_SECRET,
        });

      expect(response.status).toBe(400);
    });

    it('should reject non-integer amount', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: VALID_USER_ADDRESS,
          amount: '1.5',
          userSecret: VALID_USER_SECRET,
        });

      expect(response.status).toBe(400);
      expect(response.body.error).toContain('integer string');
    });

    it('should reject i128 amount overflow', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: VALID_USER_ADDRESS,
          amount: I128_OVERFLOW,
          userSecret: VALID_USER_SECRET,
        });

      expect(response.status).toBe(400);
      expect(response.body.error).toContain('signed 128-bit');
    });

    it('should reject missing userSecret', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: VALID_USER_ADDRESS,
          amount: '1000000',
        });

      expect(response.status).toBe(400);
    });

    it('should allow valid body with optional assetAddress', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: VALID_USER_ADDRESS,
          assetAddress: VALID_ASSET_ADDRESS,
          amount: '1000000',
          userSecret: VALID_USER_SECRET,
        });

      expect(response.status).toBe(400);
      expect(response.body.error).toBe('mock transaction failure');
      expect(mockStellarService.buildDepositTransaction).toHaveBeenCalledWith(
        VALID_USER_ADDRESS,
        VALID_ASSET_ADDRESS,
        '1000000',
        VALID_USER_SECRET
      );
    });
  });

  describe('Borrow Validation', () => {
    it('should validate all required fields', async () => {
      const response = await request(app)
        .post('/api/lending/borrow')
        .send({});

      expect(response.status).toBe(400);
    });
  });

  describe('Repay Validation', () => {
    it('should validate all required fields', async () => {
      const response = await request(app)
        .post('/api/lending/repay')
        .send({});

      expect(response.status).toBe(400);
    });
  });

  describe('Withdraw Validation', () => {
    it('should validate all required fields', async () => {
      const response = await request(app)
        .post('/api/lending/withdraw')
        .send({});

      expect(response.status).toBe(400);
    });
  });
});

describe('Hook HMAC Validation', () => {
  const mockReq = {
    headers: {},
    body: {},
    rawBody: '{}',
  } as any;
  const mockRes = {} as any;
  const next = jest.fn();

  beforeEach(() => {
    process.env.STELLAR_API_HOOK_SECRET = 'validation-hook-secret';
  });

  it('rejects missing hook headers', () => {
    jest.isolateModules(() => {
      const { verifyHookHmac } = require('../middleware/auth');
      expect(() => verifyHookHmac(mockReq, mockRes, next)).toThrow(
        'Hook signature and timestamp headers are required'
      );
    });
  });

  it('rejects invalid hook timestamp', () => {
    jest.isolateModules(() => {
      const { verifyHookHmac } = require('../middleware/auth');
      const req = {
        headers: {
          'x-hook-timestamp': 'not-a-number',
          'x-hook-signature': 'abcd',
        },
        body: {},
        rawBody: '{}',
      } as any;

      expect(() => verifyHookHmac(req, mockRes, next)).toThrow(
        'Invalid hook timestamp'
      );
    });
  });
});
