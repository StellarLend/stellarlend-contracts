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
        address: 'GABC123DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
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
      req.body = { address: 'GABC123DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO' };
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
      req.body = { address: 'XABC123DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO' };
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
      req.body = { user: 'GABC123DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO' };
      req.user = { address: 'GABC123DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO' };
      validateOwnership()(req as AuthRequest, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject when user does not own resource', () => {
      req.body = { user: 'GXYZ789ABCDEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO' };
      req.user = { address: 'GABC123DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO' };
      expect(() => validateOwnership()(req as AuthRequest, res as Response, next)).toThrow(
        "cannot modify another user's user"
      );
    });

    it('should reject when not authenticated', () => {
      req.body = { user: 'GABC123DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO' };
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
        borrower: 'GABC123DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
        liquidator: 'GXYZ789DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
        debtAsset: 'GDEF456DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
        collateralAsset: 'GHIJ789DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
        repayAmount: 1000,
      };
      validateLiquidation(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject self-liquidation', () => {
      const address = 'GABC123DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO';
      req.body = {
        borrower: address,
        liquidator: address,
        debtAsset: 'GDEF456DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
        collateralAsset: 'GHIJ789DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
        repayAmount: 1000,
      };
      expect(() => validateLiquidation(req as Request, res as Response, next)).toThrow(
        'Self-liquidation is not allowed'
      );
    });

    it('should reject missing borrower', () => {
      req.body = {
        liquidator: 'GXYZ789DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
        debtAsset: 'GDEF456DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
        collateralAsset: 'GHIJ789DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
        repayAmount: 1000,
      };
      expect(() => validateLiquidation(req as Request, res as Response, next)).toThrow(
        'Borrower address is required'
      );
    });

    it('should reject invalid repay amount', () => {
      req.body = {
        borrower: 'GABC123DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
        liquidator: 'GXYZ789DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
        debtAsset: 'GDEF456DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
        collateralAsset: 'GHIJ789DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO',
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

    it('should handle query with no dangerous characters and special chars', () => {
      req.query = { q: 'test & check = value' };
      sanitizeSearchQuery(req as Request, res as Response, next);
      expect(req.query.q).toBe('test & check = value');
    });

    it('should reject non-string query', () => {
      req.query = { q: 123 };
      expect(() => sanitizeSearchQuery(req as Request, res as Response, next)).toThrow(
        'Search query must be a string'
      );
    });
  });

  describe('validateAsset', () => {
    it('should pass for valid asset', () => {
      req.body = { asset: 'GABC123DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO' };
      validateAsset(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject invalid asset address', () => {
      req.body = { asset: 'invalid' };
      expect(() => validateAsset(req as Request, res as Response, next)).toThrow(
        ValidationError
      );
    });

    it('should reject missing asset', () => {
      req.body = {};
      expect(() => validateAsset(req as Request, res as Response, next)).toThrow(
        'asset is required'
      );
    });
  });

  describe('validateHealthFactor', () => {
    it('should pass for valid health factor', () => {
      req.body = { healthFactor: 1.5 };
      validateHealthFactor(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject health factor <= 0', () => {
      req.body = { healthFactor: 0 };
      expect(() => validateHealthFactor(req as Request, res as Response, next)).toThrow(
        'Health factor must be positive'
      );
    });

    it('should reject missing health factor', () => {
      req.body = {};
      expect(() => validateHealthFactor(req as Request, res as Response, next)).toThrow(
        'Health factor is required'
      );
    });
  });

  describe('validateTimestamp', () => {
    it('should pass for valid timestamp', () => {
      const now = Math.floor(Date.now() / 1000);
      req.body = { timestamp: now - 100 };
      validateTimestamp(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject future timestamp', () => {
      const future = Math.floor(Date.now() / 1000) + 1000;
      req.body = { timestamp: future };
      expect(() => validateTimestamp(req as Request, res as Response, next)).toThrow(
        'Timestamp cannot be in the future'
      );
    });

    it('should reject missing timestamp', () => {
      req.body = {};
      expect(() => validateTimestamp(req as Request, res as Response, next)).toThrow(
        'Timestamp is required'
      );
    });
  });

  describe('validateContractCall', () => {
    it('should pass for valid contract call', () => {
      req.body = {
        contractId: 'CA1234567890',
        method: 'transfer',
        params: { to: 'GABC123DEFGHIJKLMNOPQRSTUVWXYZ23456789ABCDEFGHIJKLMNO', amount: 1000 },
      };
      validateContractCall(req as Request, res as Response, next);
      expect(next).toHaveBeenCalled();
    });

    it('should reject missing contractId', () => {
      req.body = { method: 'transfer', params: {} };
      expect(() => validateContractCall(req as Request, res as Response, next)).toThrow(
        'contractId is required'
      );
    });

    it('should reject missing method', () => {
      req.body = { contractId: 'CA1234567890', params: {} };
      expect(() => validateContractCall(req as Request, res as Response, next)).toThrow(
        'method is required'
      );
    });

    it('should reject invalid contractId format', () => {
      req.body = { contractId: 'invalid', method: 'transfer', params: {} };
      expect(() => validateContractCall(req as Request, res as Response, next)).toThrow(
        'Invalid contract ID format'
      );
    });
  });
});
