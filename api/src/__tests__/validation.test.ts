import request from 'supertest';
import { z } from 'zod';
import {
  validateBody,
  lendingRequestSchema,
  submitRequestSchema,
} from '../middleware/validation';
import { I128String, StellarAddress } from '../utils/validators';
import { ValidationError } from '../utils/errors';

const mockStellarService = {
  buildDepositTransaction: jest.fn(),
  buildBorrowTransaction: jest.fn(),
  buildRepayTransaction: jest.fn(),
  buildWithdrawTransaction: jest.fn(),
  submitTransaction: jest.fn(),
  submitSignedTransaction: jest.fn(),
  monitorTransaction: jest.fn(),
};

jest.mock('../services/stellar.service', () => ({
  StellarService: jest.fn(() => mockStellarService),
}));

const app = require('../app').default;

const VALID_USER_ADDRESS = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ';
const VALID_CONTRACT_ADDRESS = 'CBQ4OAIKSIQS4XHACA4KH4TKQAZX4WLEARB5HWZRZPXAKMLLRRZTWI5R';
const VALID_ASSET_ADDRESS = 'GD5TFY4DYYF43CQN3UMZUPBBXBLWK3WYAM5PIOMKOVRHBTZF7J7VGHP4';
const I128_MAX = '170141183460469231731687303715884105727';
const I128_OVERFLOW = '170141183460469231731687303715884105728';
const PREPARED_TX = {
  xdr: 'prepared_tx_xdr',
  networkPassphrase: 'Test SDF Network ; September 2015',
};

describe('Validation Middleware', () => {
  beforeEach(() => {
    jest.clearAllMocks();

    mockStellarService.buildDepositTransaction.mockResolvedValue(PREPARED_TX);
    mockStellarService.buildBorrowTransaction.mockResolvedValue(PREPARED_TX);
    mockStellarService.buildRepayTransaction.mockResolvedValue(PREPARED_TX);
    mockStellarService.buildWithdrawTransaction.mockResolvedValue(PREPARED_TX);
  });

  describe('Shared Validators', () => {
    it('accepts valid Stellar account and contract addresses', () => {
      expect(StellarAddress.safeParse(VALID_USER_ADDRESS).success).toBe(true);
      expect(StellarAddress.safeParse(VALID_CONTRACT_ADDRESS).success).toBe(true);
    });

    it('rejects malformed Stellar addresses', () => {
      expect(StellarAddress.safeParse('invalid_address').success).toBe(false);
    });

    it('validates I128String bounds', () => {
      expect(I128String.safeParse(I128_MAX).success).toBe(true);
      expect(I128String.safeParse(I128_OVERFLOW).success).toBe(false);
    });
  });

  describe('validateBody', () => {
    it('replaces req.body with parsed output on success', () => {
      const schema = z.object({ value: z.string().trim() });
      const req = { body: { value: '  ok  ' }, path: '/test' } as any;
      const next = jest.fn();

      validateBody(schema)(req, {} as any, next);

      expect(req.body).toEqual({ value: 'ok' });
      expect(next).toHaveBeenCalledWith();
    });

    it('forwards ValidationError on Zod failure without mutating req.body', () => {
      const schema = z.object({ value: z.string().min(5) });
      const original = { value: 'no' };
      const req = { body: original, path: '/test' } as any;
      const next = jest.fn();

      validateBody(schema)(req, {} as any, next);

      expect(req.body).toBe(original);
      expect(next).toHaveBeenCalledWith(expect.any(ValidationError));
    });
  });

  describe('lendingRequestSchema', () => {
    it('accepts { userAddress, amount, assetAddress }', () => {
      const res = lendingRequestSchema.safeParse({
        userAddress: VALID_USER_ADDRESS,
        assetAddress: VALID_ASSET_ADDRESS,
        amount: '1000000',
      });
      expect(res.success).toBe(true);
    });

    it('rejects requests containing userSecret', () => {
      const res = lendingRequestSchema.safeParse({
        userAddress: VALID_USER_ADDRESS,
        amount: '1000000',
        userSecret: 'SAOS4OGIK6HD4QGR3DVRRDSR4FUBH73FCZGRZ7M53LRN67UQE5JDNS4I',
      });
      expect(res.success).toBe(false);
    });
  });

  describe('submitRequestSchema', () => {
    it('accepts a non-empty xdr string', () => {
      const res = submitRequestSchema.safeParse({ xdr: 'AAAA...' });
      expect(res.success).toBe(true);
    });

    it('rejects empty or missing xdr', () => {
      expect(submitRequestSchema.safeParse({}).success).toBe(false);
      expect(submitRequestSchema.safeParse({ xdr: '   ' }).success).toBe(false);
    });

    it('rejects unknown extra keys', () => {
      const res = submitRequestSchema.safeParse({
        xdr: 'AAAA...',
        userSecret: 'S...',
      });
      expect(res.success).toBe(false);
    });
  });

  describe('Lending Endpoints (/deposit, /borrow, /repay, /withdraw)', () => {
    it.each([
      ['/api/lending/deposit', 'buildDepositTransaction'],
      ['/api/lending/borrow', 'buildBorrowTransaction'],
      ['/api/lending/repay', 'buildRepayTransaction'],
      ['/api/lending/withdraw', 'buildWithdrawTransaction'],
    ] as const)('accepts valid payload on %s without userSecret', async (endpoint, method) => {
      const response = await request(app).post(endpoint).send({
        userAddress: VALID_USER_ADDRESS,
        assetAddress: VALID_ASSET_ADDRESS,
        amount: '1000000',
      });

      expect(response.status).toBe(200);
      expect(response.body).toEqual(PREPARED_TX);
      expect(mockStellarService[method]).toHaveBeenCalledWith(
        VALID_USER_ADDRESS,
        VALID_ASSET_ADDRESS,
        '1000000'
      );
    });

    it.each([
      ['/api/lending/deposit', 'buildDepositTransaction'],
      ['/api/lending/borrow', 'buildBorrowTransaction'],
      ['/api/lending/repay', 'buildRepayTransaction'],
      ['/api/lending/withdraw', 'buildWithdrawTransaction'],
    ] as const)('rejects payload containing userSecret on %s with 400', async (endpoint, method) => {
      const response = await request(app).post(endpoint).send({
        userAddress: VALID_USER_ADDRESS,
        amount: '1000000',
        userSecret: 'SAOS4OGIK6HD4QGR3DVRRDSR4FUBH73FCZGRZ7M53LRN67UQE5JDNS4I',
      });

      expect(response.status).toBe(400);
      expect(response.body.success).toBe(false);
      expect(mockStellarService[method]).not.toHaveBeenCalled();
    });

    it('rejects invalid amount (zero, negative, decimal, overflow)', async () => {
      for (const amount of ['0', '-100', '1.5', I128_OVERFLOW]) {
        const res = await request(app).post('/api/lending/deposit').send({
          userAddress: VALID_USER_ADDRESS,
          amount,
        });
        expect(res.status).toBe(400);
      }
      expect(mockStellarService.buildDepositTransaction).not.toHaveBeenCalled();
    });

    it('accepts boundary amounts 1 and i128::MAX', async () => {
      for (const amount of ['1', I128_MAX]) {
        const res = await request(app).post('/api/lending/deposit').send({
          userAddress: VALID_USER_ADDRESS,
          amount,
        });
        expect(res.status).toBe(200);
      }
    });
  });
});
