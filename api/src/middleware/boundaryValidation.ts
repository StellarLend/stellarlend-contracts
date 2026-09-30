import { Request, Response, NextFunction } from 'express';
import { ValidationError } from '../utils/errors';
import { AuthRequest } from './authorization';

/**
 * Boundary Validation Middleware
 * 
 * Enforces validation boundaries at the API layer before contract calls.
 * This provides defense-in-depth by catching invalid inputs early.
 */

// Stellar amount constraints (stroops)
const MIN_AMOUNT = 1;
const MAX_AMOUNT = 9223372036854775807n; // i128::MAX for Stellar

// Address format validation
const STELLAR_ADDRESS_REGEX = /^G[A-Z2-7]{55}$/;

/**
 * Validate amount parameters
 */
export const validateAmount = (
  req: Request,
  res: Response,
  next: NextFunction
) => {
  const body = req.body as { amount?: string | number };
  const amount = body.amount;

  if (amount === undefined || amount === null) {
    throw new ValidationError('Amount is required');
  }

  const amountNum = typeof amount === 'string' ? parseInt(amount, 10) : amount;

  if (isNaN(amountNum)) {
    throw new ValidationError('Amount must be a valid number');
  }

  if (amountNum <= 0) {
    throw new ValidationError('Amount must be positive and non-zero');
  }

  if (amountNum < MIN_AMOUNT) {
    throw new ValidationError(`Amount must be at least ${MIN_AMOUNT}`);
  }

  if (BigInt(amountNum) > MAX_AMOUNT) {
    throw new ValidationError(`Amount exceeds maximum allowed value`);
  }

  // Check for decimal places (amounts should be in stroops, no decimals)
  if (!Number.isInteger(amountNum)) {
    throw new ValidationError('Amount must be an integer (stroops)');
  }

  next();
};

/**
 * Validate Stellar address format
 */
export const validateStellarAddress = (field: string = 'address') => {
  return (req: Request, res: Response, next: NextFunction) => {
    const body = req.body as Record<string, unknown>;
    const address = body[field];

    if (!address || typeof address !== 'string') {
      throw new ValidationError(`${field} is required and must be a string`);
    }

    if (!STELLAR_ADDRESS_REGEX.test(address)) {
      throw new ValidationError(
        `Invalid Stellar address format for ${field}. Must start with G and be 56 characters.`
      );
    }

    next();
  };
};

/**
 * Validate user owns the resource they're trying to modify
 */
export const validateOwnership = (resourceField: string = 'user') => {
  return (req: AuthRequest, res: Response, next: NextFunction) => {
    const body = req.body as Record<string, unknown>;
    const resourceOwner = body[resourceField];

    if (!req.user?.address) {
      throw new ValidationError('Authentication required');
    }

    if (!resourceOwner || typeof resourceOwner !== 'string') {
      throw new ValidationError(`${resourceField} is required`);
    }

    if (req.user.address !== resourceOwner) {
      throw new ValidationError(
        `Authorization failed: cannot modify another user's ${resourceField}`
      );
    }

    next();
  };
};

/**
 * Validate network parameter matches authenticated network
 */
export const validateNetworkMatch = (
  req: AuthRequest,
  res: Response,
  next: NextFunction
) => {
  const body = req.body as { network?: string };
  const networkParam = body.network || req.params.network || req.query.network;

  if (!req.user?.network) {
    throw new ValidationError('Network context not established');
  }

  if (networkParam && networkParam !== req.user.network) {
    throw new ValidationError(
      `Network mismatch: request specifies ${networkParam} but authenticated for ${req.user.network}`
    );
  }

  next();
};

/**
 * Validate asset address
 */
export const validateAsset = (
  req: Request,
  res: Response,
  next: NextFunction
) => {
  const body = req.body as { asset?: string };
  const asset = body.asset;

  if (!asset || typeof asset !== 'string') {
    throw new ValidationError('Asset address is required');
  }

  if (!STELLAR_ADDRESS_REGEX.test(asset)) {
    throw new ValidationError('Invalid asset address format');
  }

  next();
};

/**
 * Validate health factor (basis points, 0-100000)
 */
export const validateHealthFactor = (
  req: Request,
  res: Response,
  next: NextFunction
) => {
  const body = req.body as { healthFactor?: number };
  const healthFactor = body.healthFactor;

  if (healthFactor === undefined) {
    // Health factor is optional in requests, calculated on-chain
    return next();
  }

  if (typeof healthFactor !== 'number' || isNaN(healthFactor)) {
    throw new ValidationError('Health factor must be a valid number');
  }

  if (healthFactor < 0) {
    throw new ValidationError('Health factor cannot be negative');
  }

  // Health factor > 1.0 (10000 basis points) means healthy
  // This is informational validation, actual enforcement is on-chain
  if (healthFactor < 10000) {
    console.warn(`Low health factor detected: ${healthFactor} (below 1.0)`);
  }

  next();
};

/**
 * Validate timestamp is within acceptable range
 */
export const validateTimestamp = (field: string = 'timestamp') => {
  return (req: Request, res: Response, next: NextFunction) => {
    const body = req.body as Record<string, unknown>;
    const timestamp = body[field];

    if (!timestamp) {
      // Timestamp might be optional, add if needed
      return next();
    }

    const ts = typeof timestamp === 'string' ? parseInt(timestamp, 10) : timestamp;

    if (typeof ts !== 'number' || isNaN(ts)) {
      throw new ValidationError(`${field} must be a valid number`);
    }

    const now = Math.floor(Date.now() / 1000);
    const maxDeviation = 300; // 5 minutes

    if (Math.abs(now - ts) > maxDeviation) {
      throw new ValidationError(
        `${field} is outside acceptable range (more than ${maxDeviation}s from current time)`
      );
    }

    next();
  };
};

/**
 * Validate oracle price data
 */
export const validateOraclePrice = (
  req: Request,
  res: Response,
  next: NextFunction
) => {
  const body = req.body as {
    price?: number;
    priceTimestamp?: number;
    signature?: string;
  };

  if (!body.price || !body.priceTimestamp) {
    throw new ValidationError('Price and priceTimestamp are required for oracle data');
  }

  if (body.price <= 0) {
    throw new ValidationError('Price must be positive');
  }

  // Validate price timestamp freshness
  const now = Math.floor(Date.now() / 1000);
  const maxAge = 3600; // 1 hour

  if (body.priceTimestamp > now) {
    throw new ValidationError('Price timestamp cannot be in the future');
  }

  if (now - body.priceTimestamp > maxAge) {
    throw new ValidationError('Price data is stale (older than 1 hour)');
  }

  // Validate signature if provided
  if (body.signature && typeof body.signature !== 'string') {
    throw new ValidationError('Signature must be a string');
  }

  next();
};

/**
 * Validate liquidation parameters
 */
export const validateLiquidation = (
  req: Request,
  res: Response,
  next: NextFunction
) => {
  const body = req.body as {
    borrower?: string;
    liquidator?: string;
    debtAsset?: string;
    collateralAsset?: string;
    repayAmount?: number;
  };

  // Validate required fields
  if (!body.borrower) {
    throw new ValidationError('Borrower address is required');
  }

  if (!body.liquidator) {
    throw new ValidationError('Liquidator address is required');
  }

  if (!body.debtAsset) {
    throw new ValidationError('Debt asset is required');
  }

  if (!body.collateralAsset) {
    throw new ValidationError('Collateral asset is required');
  }

  if (!body.repayAmount || body.repayAmount <= 0) {
    throw new ValidationError('Repay amount must be positive');
  }

  // Validate addresses
  [body.borrower, body.liquidator, body.debtAsset, body.collateralAsset].forEach((addr, idx) => {
    const field = ['borrower', 'liquidator', 'debtAsset', 'collateralAsset'][idx];
    if (addr && !STELLAR_ADDRESS_REGEX.test(addr)) {
      throw new ValidationError(`Invalid ${field} address format`);
    }
  });

  // Prevent self-liquidation
  if (body.borrower === body.liquidator) {
    throw new ValidationError('Self-liquidation is not allowed');
  }

  next();
};

/**
 * Validate pagination parameters
 */
export const validatePagination = (
  req: Request,
  res: Response,
  next: NextFunction
) => {
  const page = parseInt(req.query.page as string) || 1;
  const limit = parseInt(req.query.limit as string) || 10;

  if (page < 1) {
    throw new ValidationError('Page must be >= 1');
  }

  if (limit < 1 || limit > 100) {
    throw new ValidationError('Limit must be between 1 and 100');
  }

  // Attach validated pagination to request
  (req as AuthRequest & { pagination?: { page: number; limit: number } }).pagination = {
    page,
    limit,
  };

  next();
};

/**
 * Validate rate parameters (basis points)
 */
export const validateRateParams = (
  req: Request,
  res: Response,
  next: NextFunction
) => {
  const body = req.body as {
    baseRateBps?: number;
    optimalUtilizationBps?: number;
    slopeRateBps?: number;
  };

  const bpsFields = [
    { field: 'baseRateBps', value: body.baseRateBps },
    { field: 'optimalUtilizationBps', value: body.optimalUtilizationBps },
    { field: 'slopeRateBps', value: body.slopeRateBps },
  ];

  for (const { field, value } of bpsFields) {
    if (value !== undefined) {
      if (typeof value !== 'number' || isNaN(value)) {
        throw new ValidationError(`${field} must be a valid number`);
      }

      if (value < 0 || value > 10000) {
        throw new ValidationError(`${field} must be between 0 and 10000 (basis points)`);
      }
    }
  }

  next();
};

/**
 * Sanitize and validate search query to prevent injection
 */
export const sanitizeSearchQuery = (
  req: Request,
  res: Response,
  next: NextFunction
) => {
  const query = req.query.q as string;

  if (!query) {
    return next();
  }

  // Remove potentially dangerous characters
  const sanitized = query
    .replace(/[<>\"'%;()&+]/g, '')
    .trim()
    .substring(0, 100); // Limit length

  if (sanitized !== query) {
    console.warn(`Search query was sanitized: "${query}" -> "${sanitized}"`);
  }

  req.query.q = sanitized;
  next();
};

/**
 * Validate contract call parameters comprehensively
 */
export const validateContractCall = (
  req: AuthRequest,
  res: Response,
  next: NextFunction
) => {
  const body = req.body as {
    contractId?: string;
    functionName?: string;
    args?: unknown[];
  };

  if (!body.contractId) {
    throw new ValidationError('Contract ID is required');
  }

  if (!body.functionName) {
    throw new ValidationError('Function name is required');
  }

  // Validate contract ID format (Stellar contract addresses start with C)
  if (!body.contractId.startsWith('C') || body.contractId.length !== 56) {
    throw new ValidationError('Invalid contract ID format');
  }

  // Validate function name (alphanumeric and underscore only)
  if (!/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(body.functionName)) {
    throw new ValidationError('Invalid function name format');
  }

  // Validate args is an array if provided
  if (body.args && !Array.isArray(body.args)) {
    throw new ValidationError('Contract arguments must be an array');
  }

  next();
};

/**
 * Combined validation middleware for common operations
 */
export const validateDepositRequest = [
  validateAmount,
  validateStellarAddress('user'),
  validateAsset,
  validateNetworkMatch,
];

export const validateWithdrawRequest = [
  validateAmount,
  validateStellarAddress('user'),
  validateAsset,
  validateNetworkMatch,
  validateOwnership('user'),
];

export const validateBorrowRequest = [
  validateAmount,
  validateStellarAddress('user'),
  validateAsset,
  validateNetworkMatch,
  validateOwnership('user'),
];

export const validateRepayRequest = [
  validateAmount,
  validateStellarAddress('user'),
  validateAsset,
  validateNetworkMatch,
];

export const validateLiquidationRequest = [
  validateLiquidation,
  validateNetworkMatch,
];
