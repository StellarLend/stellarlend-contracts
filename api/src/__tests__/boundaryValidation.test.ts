import { Request, Response, NextFunction } from 'express';
import {
  validateAmount,
  validateStellarAddress,
  validateOwnership,
  validateNetworkMatch,
  validateAsset,
  validateHealthFactor,
  validateTimestamp,
  validateOraclePrice,
  validateLiquidation,
  validatePagination,
  validateRateParams,
  sanitizeSearchQuery,
  validateContractCall,
} from '../middleware/boundaryValidation';
import { ValidationError } from '../utils/errors';
import { AuthRequest } from '../middleware/authorization';

const VALID_ADDRESS = 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK';
const VALID_ADDRESS_2 = 'GD5TFY4DYYF43CQN3UMZUPBBXBLWK3WYAM5PIOMKOVRHBTZF7J7VGHP4';
const VALID_CONTRACT_ID = 'CADQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQP5KR';

describe('Boundary Validation Middleware', () => {
  let req: Partial<AuthRequest>;
  let res: Partial<Response>;
  let next: NextFunction;

  beforeEach(() => {
    req = {
      body: {},
      params: {},
      query: {},
      user: {
        address: 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK',
        network: 'testnet',
      },
    };
    res = {};
    next = jest.fn();
  });

  describe('validateAmount', () => {
    it('should pass for valid positive amount', () => {
      req.body = { amount: 1000 };
      validateAmount(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject zero amount', () => {
      req.body = { amount: 0 };
      expect(() => validateAmount(req as Request, res as Response, next)).toThrow(
        ValidationError
      );
      expect(() => validateAmount(req as Request, res as Response, next)).toThrow(
        'Amount must be positive and non-zero'
      );
    });

    it('should reject negative amount', () => {
      req.body = { amount: -100 };
      expect(() => validateAmount(req as Request, res as Response, next)).toThrow(
        ValidationError
      );
    });

    it('should reject non-integer amount', () => {
      req.body = { amount: 100.5 };
      expect(() => validateAmount(req as Request, res as Response, next)).toThrow(
        'Amount must be an integer'
      );
    });

    it('should reject missing amount', () => {
      req.body = {};
      expect(() => validateAmount(req as Request, res as Response, next)).toThrow(
        'Amount is required'
      );
    });

    it('should reject NaN amount', () => {
      req.body = { amount: 'invalid' };
      expect(() => validateAmount(req as Request, res as Response, next)).toThrow(
        'Amount must be a valid number'
      );
    });
  });

  describe('validateStellarAddress', () => {
    it('should pass for valid Stellar address', () => {
      req.body = { address: 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK' };
      validateStellarAddress()(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject invalid address format', () => {
      req.body = { address: 'invalid' };
      expect(() => validateStellarAddress()(req as Request, res as Response, next)).toThrow(
        'Invalid Stellar address format'
      );
    });

    it('should reject address not starting with G', () => {
      req.body = { address: 'XABC123DEFGHIJKLMNOPQRSTUVWXYZ234567890ABCDEFGHIJKLMNO' };
      expect(() => validateStellarAddress()(req as Request, res as Response, next)).toThrow(
        ValidationError
      );
    });

    it('should reject address with wrong length', () => {
      req.body = { address: 'GABC123' };
      expect(() => validateStellarAddress()(req as Request, res as Response, next)).toThrow(
        ValidationError
      );
    });

    it('should reject missing address', () => {
      req.body = {};
      expect(() => validateStellarAddress()(req as Request, res as Response, next)).toThrow(
        'address is required'
      );
    });
  });

  describe('validateOwnership', () => {
    it('should pass when user owns resource', () => {
      req.body = { user: 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK' };
      req.user = { address: 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK' };
      validateOwnership()(req as AuthRequest, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject when user does not own resource', () => {
      req.body = { user: 'GXYZ789ABCDEFGHIJKLMNOPQRSTUVWXYZ234567890ABCDEFGHIJKLMNO' };
      req.user = { address: 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK' };
      expect(() => validateOwnership()(req as AuthRequest, res as Response, next)).toThrow(
        "cannot modify another user's user"
      );
    });

    it('should reject when not authenticated', () => {
      req.body = { user: 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK' };
      req.user = undefined;
      expect(() => validateOwnership()(req as AuthRequest, res as Response, next)).toThrow(
        'Authentication required'
      );
    });
  });

  describe('validateNetworkMatch', () => {
    it('should pass when networks match', () => {
      req.body = { network: 'testnet' };
      req.user = { address: 'GABC...', network: 'testnet' };
      validateNetworkMatch(req as AuthRequest, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject network mismatch', () => {
      req.body = { network: 'public' };
      req.user = { address: 'GABC...', network: 'testnet' };
      expect(() => validateNetworkMatch(req as AuthRequest, res as Response, next)).toThrow(
        'Network mismatch'
      );
    });

    it('should reject when network context not established', () => {
      req.body = { network: 'testnet' };
      req.user = { address: 'GABC...' };
      expect(() => validateNetworkMatch(req as AuthRequest, res as Response, next)).toThrow(
        'Network context not established'
      );
    });
  });

  describe('validateOraclePrice', () => {
    it('should pass for valid oracle data', () => {
      const now = Math.floor(Date.now() / 1000);
      req.body = {
        price: 1000000,
        priceTimestamp: now - 100,
        signature: 'valid_signature',
      };
      validateOraclePrice(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject negative price', () => {
      const now = Math.floor(Date.now() / 1000);
      req.body = {
        price: -1000,
        priceTimestamp: now,
      };
      expect(() => validateOraclePrice(req as Request, res as Response, next)).toThrow(
        'Price must be positive'
      );
    });

    it('should reject future timestamp', () => {
      const future = Math.floor(Date.now() / 1000) + 1000;
      req.body = {
        price: 1000000,
        priceTimestamp: future,
      };
      expect(() => validateOraclePrice(req as Request, res as Response, next)).toThrow(
        'Price timestamp cannot be in the future'
      );
    });

    it('should reject stale price data', () => {
      const stale = Math.floor(Date.now() / 1000) - 7200; // 2 hours ago
      req.body = {
        price: 1000000,
        priceTimestamp: stale,
      };
      expect(() => validateOraclePrice(req as Request, res as Response, next)).toThrow(
        'Price data is stale'
      );
    });

    it('should reject missing price', () => {
      req.body = { priceTimestamp: Math.floor(Date.now() / 1000) };
      expect(() => validateOraclePrice(req as Request, res as Response, next)).toThrow(
        'Price and priceTimestamp are required'
      );
    });
  });

  describe('validateLiquidation', () => {
    it('should pass for valid liquidation request', () => {
      req.body = {
        borrower: 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK',
        liquidator: 'GD6DRYD5CQP3TNK2FMTDWD3CVX36F2JOHGEHO4Q3CYVD6A4Z6YLG53YR',
        debtAsset: 'GBGGEQ672BQWOQHIK2IZPWV653P3VXG2TC2LKZTCBDM2Q3R3YMV7ECOS',
        collateralAsset: 'GAH3NM2JUYAEADH5EPJJ77X67KVCVPTH2JND62NDRYIJSIP45XGZBOEN',
        repayAmount: 1000,
      };
      validateLiquidation(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject self-liquidation', () => {
      const address = 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK';
      req.body = {
        borrower: address,
        liquidator: address,
        debtAsset: 'GBGGEQ672BQWOQHIK2IZPWV653P3VXG2TC2LKZTCBDM2Q3R3YMV7ECOS',
        collateralAsset: 'GAH3NM2JUYAEADH5EPJJ77X67KVCVPTH2JND62NDRYIJSIP45XGZBOEN',
        repayAmount: 1000,
      };
      expect(() => validateLiquidation(req as Request, res as Response, next)).toThrow(
        'Self-liquidation is not allowed'
      );
    });

    it('should reject missing borrower', () => {
      req.body = {
        liquidator: 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK',
        debtAsset: 'GBGGEQ672BQWOQHIK2IZPWV653P3VXG2TC2LKZTCBDM2Q3R3YMV7ECOS',
        collateralAsset: 'GAH3NM2JUYAEADH5EPJJ77X67KVCVPTH2JND62NDRYIJSIP45XGZBOEN',
        repayAmount: 1000,
      };
      expect(() => validateLiquidation(req as Request, res as Response, next)).toThrow(
        'Borrower address is required'
      );
    });

    it('should reject invalid repay amount', () => {
      req.body = {
        borrower: 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK',
        liquidator: 'GD6DRYD5CQP3TNK2FMTDWD3CVX36F2JOHGEHO4Q3CYVD6A4Z6YLG53YR',
        debtAsset: 'GBGGEQ672BQWOQHIK2IZPWV653P3VXG2TC2LKZTCBDM2Q3R3YMV7ECOS',
        collateralAsset: 'GAH3NM2JUYAEADH5EPJJ77X67KVCVPTH2JND62NDRYIJSIP45XGZBOEN',
        repayAmount: 0,
      };
      expect(() => validateLiquidation(req as Request, res as Response, next)).toThrow(
        'Repay amount must be positive'
      );
    });
  });

  describe('validatePagination', () => {
    it('should pass and set pagination for valid params', () => {
      req.query = { page: '2', limit: '20' };
      validatePagination(req as Request, res as Response, next);
      expect((req as AuthRequest & { pagination?: unknown }).pagination).toEqual({
        page: 2,
        limit: 20,
      });
      expect(next).toHaveBeenCalled();
    });

    it('should use defaults for missing params', () => {
      req.query = {};
      validatePagination(req as Request, res as Response, next);
      expect((req as AuthRequest & { pagination?: unknown }).pagination).toEqual({
        page: 1,
        limit: 10,
      });
    });

    it('should reject page < 1', () => {
      req.query = { page: '0' };
      expect(() => validatePagination(req as Request, res as Response, next)).toThrow(
        'Page must be >= 1'
      );
    });

    it('should reject limit > 100', () => {
      req.query = { limit: '101' };
      expect(() => validatePagination(req as Request, res as Response, next)).toThrow(
        'Limit must be between 1 and 100'
      );
    });
  });

  describe('validateRateParams', () => {
    it('should pass for valid basis points', () => {
      req.body = {
        baseRateBps: 500,
        optimalUtilizationBps: 8000,
        slopeRateBps: 2000,
      };
      validateRateParams(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject basis points > 10000', () => {
      req.body = { baseRateBps: 15000 };
      expect(() => validateRateParams(req as Request, res as Response, next)).toThrow(
        'baseRateBps must be between 0 and 10000'
      );
    });

    it('should reject negative basis points', () => {
      req.body = { optimalUtilizationBps: -100 };
      expect(() => validateRateParams(req as Request, res as Response, next)).toThrow(
        'optimalUtilizationBps must be between 0 and 10000'
      );
    });
  });

  describe('sanitizeSearchQuery', () => {
    it('should pass clean queries unchanged', () => {
      req.query = { q: 'safe query' };
      sanitizeSearchQuery(req as Request, res as Response, next);
      expect(req.query.q).toBe('safe query');
      expect(next).toHaveBeenCalled();
    });

    it('should remove dangerous characters', () => {
      req.query = { q: '<script>alert("xss")</script>' };
      sanitizeSearchQuery(req as Request, res as Response, next);
      expect(req.query.q).toBe('scriptalertxss/script');
    });

    it('should limit query length', () => {
      req.query = { q: 'a'.repeat(200) };
      sanitizeSearchQuery(req as Request, res as Response, next);
      expect((req.query.q as string).length).toBe(100);
    });
  });

  describe('validateContractCall', () => {
    it('should pass for valid contract call', () => {
      req.body = {
        contractId: 'CADQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQP5KR',
        functionName: 'deposit',
        args: [100, 'GXYZ...'],
      };
      validateContractCall(req as AuthRequest, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject invalid contract ID', () => {
      req.body = {
        contractId: 'GABC123...', // Should start with C, not G
        functionName: 'deposit',
      };
      expect(() => validateContractCall(req as AuthRequest, res as Response, next)).toThrow(
        'Invalid contract ID format'
      );
    });

    it('should reject invalid function name', () => {
      req.body = {
        contractId: 'CADQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQP5KR',
        functionName: 'invalid-function!',
      };
      expect(() => validateContractCall(req as AuthRequest, res as Response, next)).toThrow(
        'Invalid function name format'
      );
    });

    it('should reject non-array args', () => {
      req.body = {
        contractId: 'CADQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQP5KR',
        functionName: 'deposit',
        args: 'not an array',
      };
      expect(() => validateContractCall(req as AuthRequest, res as Response, next)).toThrow(
        'Contract arguments must be an array'
      );
    });

    it('should reject a missing contract ID', () => {
      req.body = { functionName: 'deposit' };
      expect(() => validateContractCall(req as AuthRequest, res as Response, next)).toThrow(
        'Contract ID is required'
      );
    });

    it('should reject a missing function name', () => {
      req.body = { contractId: VALID_CONTRACT_ID };
      expect(() => validateContractCall(req as AuthRequest, res as Response, next)).toThrow(
        'Function name is required'
      );
    });
  });

  describe('validateAsset', () => {
    it('should pass for a well-formed asset address', () => {
      req.body = { asset: VALID_ADDRESS };
      validateAsset(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject a missing asset', () => {
      req.body = {};
      expect(() => validateAsset(req as Request, res as Response, next)).toThrow(
        'Asset address is required'
      );
    });

    it('should reject a non-string asset', () => {
      req.body = { asset: 12345 };
      expect(() => validateAsset(req as Request, res as Response, next)).toThrow(
        'Asset address is required'
      );
    });

    it('should reject a malformed asset address', () => {
      req.body = { asset: 'not-a-stellar-address' };
      expect(() => validateAsset(req as Request, res as Response, next)).toThrow(
        'Invalid asset address format'
      );
    });
  });

  describe('validateHealthFactor', () => {
    it('should pass when the health factor is omitted', () => {
      req.body = {};
      validateHealthFactor(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should pass for a healthy factor', () => {
      req.body = { healthFactor: 15000 };
      validateHealthFactor(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should pass but warn for a factor below 1.0', () => {
      req.body = { healthFactor: 5000 };
      validateHealthFactor(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject a non-numeric health factor', () => {
      req.body = { healthFactor: 'high' };
      expect(() => validateHealthFactor(req as Request, res as Response, next)).toThrow(
        'Health factor must be a valid number'
      );
    });

    it('should reject a negative health factor', () => {
      req.body = { healthFactor: -1 };
      expect(() => validateHealthFactor(req as Request, res as Response, next)).toThrow(
        'Health factor cannot be negative'
      );
    });
  });

  describe('validateTimestamp', () => {
    const nowSeconds = () => Math.floor(Date.now() / 1000);

    it('should pass when the timestamp is omitted', () => {
      req.body = {};
      validateTimestamp()(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should pass for a numeric timestamp inside the window', () => {
      req.body = { timestamp: nowSeconds() };
      validateTimestamp()(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should accept a numeric string timestamp', () => {
      req.body = { timestamp: String(nowSeconds()) };
      validateTimestamp()(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should read a custom field name', () => {
      req.body = { expiresAt: nowSeconds() };
      validateTimestamp('expiresAt')(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject a non-numeric timestamp', () => {
      req.body = { timestamp: 'soon' };
      expect(() => validateTimestamp()(req as Request, res as Response, next)).toThrow(
        'timestamp must be a valid number'
      );
    });

    it('should reject a timestamp outside the 5 minute window', () => {
      req.body = { timestamp: nowSeconds() - 3600 };
      expect(() => validateTimestamp()(req as Request, res as Response, next)).toThrow(
        'timestamp is outside acceptable range'
      );
    });

    it('should reject a timestamp too far in the future', () => {
      req.body = { timestamp: nowSeconds() + 3600 };
      expect(() => validateTimestamp()(req as Request, res as Response, next)).toThrow(
        'timestamp is outside acceptable range'
      );
    });
  });

  describe('additional boundary branches', () => {
    it('validateOwnership rejects a missing resource owner', () => {
      req.body = {};
      expect(() => validateOwnership()(req as AuthRequest, res as Response, next)).toThrow(
        'user is required'
      );
    });

    it('validateOwnership requires an authenticated user', () => {
      req.user = undefined;
      expect(() => validateOwnership()(req as AuthRequest, res as Response, next)).toThrow(
        'Authentication required'
      );
    });

    it('validateOraclePrice rejects a non-string signature', () => {
      const now = Math.floor(Date.now() / 1000);
      req.body = { price: 100, priceTimestamp: now, signature: 12345 };
      expect(() => validateOraclePrice(req as Request, res as Response, next)).toThrow(
        'Signature must be a string'
      );
    });

    it.each([
      ['liquidator', 'Liquidator address is required'],
      ['debtAsset', 'Debt asset is required'],
      ['collateralAsset', 'Collateral asset is required'],
    ])('validateLiquidation requires %s', (field, message) => {
      req.body = {
        borrower: VALID_ADDRESS,
        liquidator: VALID_ADDRESS_2,
        debtAsset: VALID_ADDRESS,
        collateralAsset: VALID_ADDRESS_2,
        repayAmount: 100,
        [field]: undefined,
      };
      expect(() => validateLiquidation(req as Request, res as Response, next)).toThrow(message);
    });

    it('validateLiquidation rejects a malformed address', () => {
      req.body = {
        borrower: 'bad-address',
        liquidator: VALID_ADDRESS_2,
        debtAsset: VALID_ADDRESS,
        collateralAsset: VALID_ADDRESS_2,
        repayAmount: 100,
      };
      expect(() => validateLiquidation(req as Request, res as Response, next)).toThrow(
        'Invalid borrower address format'
      );
    });

    it('validatePagination rejects a non-numeric page', () => {
      req.query = { page: 'first' };
      expect(() => validatePagination(req as Request, res as Response, next)).toThrow(
        'Page must be a valid number'
      );
    });

    it('validatePagination rejects a non-numeric limit', () => {
      req.query = { limit: 'many' };
      expect(() => validatePagination(req as Request, res as Response, next)).toThrow(
        'Limit must be a valid number'
      );
    });

    it('validateRateParams rejects a non-numeric bps field', () => {
      req.body = { slopeRateBps: '2000' };
      expect(() => validateRateParams(req as Request, res as Response, next)).toThrow(
        'slopeRateBps must be a valid number'
      );
    });

    it('sanitizeSearchQuery passes an absent query through', () => {
      req.query = {};
      sanitizeSearchQuery(req as Request, res as Response, next);
      expect(req.query.q).toBeUndefined();
      expect(next).toHaveBeenCalled();
    });
  });
});
