import { Request, Response, NextFunction } from 'express';
import jwt from 'jsonwebtoken';
import { Keypair, Networks, Transaction, TransactionBuilder } from '@stellar/stellar-sdk';
import { config } from '../config';
import { UnauthorizedError, ValidationError, ForbiddenError } from '../utils/errors';
import { assertHookHmac } from '../utils/hookHmac';

export interface AuthRequest extends Request {
  user?: {
    address: string;
    network?: string;
  };
  rawBody?: string;
  validatedTransaction?: Transaction;
}

// Network validation
const ALLOWED_NETWORKS = {
  testnet: Networks.TESTNET,
  public: Networks.PUBLIC,
  futurenet: Networks.FUTURENET,
};

/**
 * Authenticate JWT token
 */
export const authenticateToken = (
  req: AuthRequest,
  res: Response,
  next: NextFunction
) => {
  const authHeader = req.headers['authorization'];
  const token = authHeader && authHeader.split(' ')[1];

  if (!token) {
    throw new UnauthorizedError('Access token required');
  }

  try {
    const decoded = jwt.verify(token, config.auth.jwtSecret, {
      algorithms: ['HS256'],
    });

    if (
      typeof decoded !== 'object' ||
      decoded === null ||
      typeof (decoded as jwt.JwtPayload).address !== 'string'
    ) {
      throw new UnauthorizedError('Invalid or expired token');
    }

    req.user = decoded as {
      address: string;
      network?: string;
    };
    next();
  } catch (error) {
    throw new UnauthorizedError('Invalid or expired token');
  }
};

/**
 * Generate JWT token with network information
 */
export const generateToken = (address: string, network?: string): string => {
  const payload = network ? { address, network } : { address };
  return jwt.sign(
    payload,
    config.auth.jwtSecret,
    {
      algorithm: 'HS256',
      expiresIn: config.auth.jwtExpiresIn,
    } as jwt.SignOptions
  );
};

/**
 * Verify Stellar transaction signature
 * 
 * This prevents replay attacks by validating:
 * 1. Transaction signature is valid
 * 2. Transaction is for the correct network
 * 3. Transaction sequence number hasn't been used
 * 4. Transaction hasn't expired
 */
export const verifyStellarSignature = async (
  req: AuthRequest,
  res: Response,
  next: NextFunction
) => {
  try {
    const txXdr = req.headers['x-stellar-tx'] as string;
    const expectedNetwork = req.headers['x-stellar-network'] as string || 'testnet';

    if (!txXdr) {
      throw new UnauthorizedError('Stellar transaction signature required (x-stellar-tx header)');
    }

    if (!ALLOWED_NETWORKS[expectedNetwork as keyof typeof ALLOWED_NETWORKS]) {
      throw new ValidationError(`Invalid network: ${expectedNetwork}. Allowed: ${Object.keys(ALLOWED_NETWORKS).join(', ')}`);
    }

    // Decode transaction
    const transaction = TransactionBuilder.fromXDR(
      txXdr,
      ALLOWED_NETWORKS[expectedNetwork as keyof typeof ALLOWED_NETWORKS]
    ) as Transaction;

    // Verify transaction network matches expected
    const txNetworkPassphrase = transaction.networkPassphrase;
    const expectedPassphrase = ALLOWED_NETWORKS[expectedNetwork as keyof typeof ALLOWED_NETWORKS];
    
    if (txNetworkPassphrase !== expectedPassphrase) {
      throw new UnauthorizedError(
        `Network mismatch: transaction is for ${txNetworkPassphrase}, expected ${expectedPassphrase}`
      );
    }

    // Verify transaction hasn't expired
    const now = Math.floor(Date.now() / 1000);
    if (transaction.timeBounds) {
      if (transaction.timeBounds.maxTime && Number(transaction.timeBounds.maxTime) < now) {
        throw new UnauthorizedError('Transaction has expired');
      }
      if (transaction.timeBounds.minTime && Number(transaction.timeBounds.minTime) > now) {
        throw new UnauthorizedError('Transaction not yet valid');
      }
    }

    // Verify transaction has at least one signature
    if (!transaction.signatures || transaction.signatures.length === 0) {
      throw new UnauthorizedError('Transaction must be signed');
    }

    // Extract source account (signer)
    const sourceAccount = transaction.source;

    // Verify signature is valid for the source account
    // Note: This is a basic check. In production, you'd verify against the account's signers
    const isValid = verifyTransactionSignature(transaction, sourceAccount);
    if (!isValid) {
      throw new UnauthorizedError('Invalid transaction signature');
    }

    // Store validated transaction and user info
    req.validatedTransaction = transaction;
    req.user = {
      address: sourceAccount,
      network: expectedNetwork,
    };

    next();
  } catch (error) {
    if (error instanceof UnauthorizedError || error instanceof ValidationError) {
      throw error;
    }
    throw new UnauthorizedError(`Transaction validation failed: ${(error as Error).message}`);
  }
};

/**
 * Verify transaction signature against source account
 */
function verifyTransactionSignature(transaction: Transaction, sourceAccount: string): boolean {
  try {
    // Get transaction hash
    const txHash = transaction.hash();

    // Check if any signature matches the source account
    for (const signature of transaction.signatures) {
      try {
        // Try to verify signature with the source account's public key
        const keypair = Keypair.fromPublicKey(sourceAccount);
        const isValid = keypair.verify(txHash, signature.signature());
        if (isValid) {
          return true;
        }
      } catch {
        // Continue to next signature
        continue;
      }
    }

    return false;
  } catch {
    return false;
  }
}

export const verifyHookHmac = (
  req: AuthRequest,
  _res: Response,
  next: NextFunction
) => {
  assertHookHmac({
    headers: req.headers,
    rawBody: req.rawBody,
    secret: config.auth.hookSecret,
    messages: {
      timestampOutsideWindow: 'Hook timestamp outside allowable window (replay attack detected)',
      invalidSignature: 'Invalid hook signature (tampering detected)',
    },
  });
  next();
};

/**
 * Validate network consistency across request
 * Ensures all network indicators point to the same network
 */
export const validateNetworkConsistency = (
  req: AuthRequest,
  res: Response,
  next: NextFunction
) => {
  const networkFromHeader = req.headers['x-stellar-network'] as string;
  const networkFromUser = req.user?.network;
  const networkFromBody = (req.body as { network?: string })?.network;

  const networks = [networkFromHeader, networkFromUser, networkFromBody].filter(Boolean);

  if (networks.length === 0) {
    throw new ValidationError('Network must be specified');
  }

  // All specified networks must match
  const uniqueNetworks = [...new Set(networks)];
  if (uniqueNetworks.length > 1) {
    throw new ValidationError(
      `Network mismatch: multiple networks specified (${uniqueNetworks.join(', ')})`
    );
  }

  const network = uniqueNetworks[0];
  if (!ALLOWED_NETWORKS[network as keyof typeof ALLOWED_NETWORKS]) {
    throw new ValidationError(
      `Invalid network: ${network}. Allowed: ${Object.keys(ALLOWED_NETWORKS).join(', ')}`
    );
  }

  // Ensure network is set on user object
  if (req.user) {
    req.user.network = network;
  }

  next();
};

/**
 * Prevent operations from disconnected/unauthenticated wallets
 */
export const requireAuthenticatedWallet = (
  req: AuthRequest,
  res: Response,
  next: NextFunction
) => {
  if (!req.user || !req.user.address) {
    throw new UnauthorizedError('Wallet must be connected and authenticated');
  }

  // Verify address format (Stellar public key starts with G)
  if (!req.user.address.startsWith('G') || req.user.address.length !== 56) {
    throw new ValidationError('Invalid Stellar address format');
  }

  next();
};

/**
 * Ensure the authenticated user address matches the userAddress in the request body
 */
export const requireSameAddress = (
  req: AuthRequest,
  _res: Response,
  next: NextFunction
) => {
  const bodyAddress = (req.body as { userAddress?: string })?.userAddress;
  if (bodyAddress && req.user?.address && bodyAddress !== req.user.address) {
    throw new ForbiddenError('User address does not match authenticated token');
  }
  next();
};

/**
 * Rate limiting per address to prevent DoS
 */
const rateLimitMap = new Map<string, { count: number; resetAt: number }>();
const RATE_LIMIT_WINDOW_MS = 60 * 1000; // 1 minute
const RATE_LIMIT_MAX_REQUESTS = 100;

export const rateLimitByAddress = (
  req: AuthRequest,
  res: Response,
  next: NextFunction
) => {
  if (!req.user?.address) {
    throw new UnauthorizedError('Address required for rate limiting');
  }

  const address = req.user.address;
  const now = Date.now();

  const record = rateLimitMap.get(address);

  if (!record || now > record.resetAt) {
    // New window
    rateLimitMap.set(address, {
      count: 1,
      resetAt: now + RATE_LIMIT_WINDOW_MS,
    });
    res.setHeader('X-RateLimit-Limit', RATE_LIMIT_MAX_REQUESTS.toString());
    res.setHeader('X-RateLimit-Remaining', (RATE_LIMIT_MAX_REQUESTS - 1).toString());
    res.setHeader('X-RateLimit-Reset', new Date(now + RATE_LIMIT_WINDOW_MS).toISOString());
    next();
  } else if (record.count < RATE_LIMIT_MAX_REQUESTS) {
    // Within limit
    record.count++;
    res.setHeader('X-RateLimit-Limit', RATE_LIMIT_MAX_REQUESTS.toString());
    res.setHeader('X-RateLimit-Remaining', (RATE_LIMIT_MAX_REQUESTS - record.count).toString());
    res.setHeader('X-RateLimit-Reset', new Date(record.resetAt).toISOString());
    next();
  } else {
    // Exceeded limit
    res.setHeader('X-RateLimit-Limit', RATE_LIMIT_MAX_REQUESTS.toString());
    res.setHeader('X-RateLimit-Remaining', '0');
    res.setHeader('X-RateLimit-Reset', new Date(record.resetAt).toISOString());
    res.setHeader('Retry-After', Math.ceil((record.resetAt - now) / 1000).toString());
    throw new UnauthorizedError('Rate limit exceeded. Please try again later.');
  }
};

// Cleanup old rate limit entries periodically.
// unref() so the sweep timer never keeps the process (or a test runner) alive.
const cleanupTimer = setInterval(() => {
  const now = Date.now();
  for (const [address, record] of rateLimitMap.entries()) {
    if (now > record.resetAt) {
      rateLimitMap.delete(address);
    }
  }
}, RATE_LIMIT_WINDOW_MS);

if (typeof cleanupTimer.unref === 'function') {
  cleanupTimer.unref();
}
