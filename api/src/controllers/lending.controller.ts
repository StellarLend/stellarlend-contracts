import { Request, Response, NextFunction } from 'express';
import { StellarService } from '../services/stellar.service';
import {
  DepositRequest,
  BorrowRequest,
  RepayRequest,
  WithdrawRequest,
  SubmitTransactionRequest,
} from '../types';
import { AuthRequest } from '../middleware/auth';
import { UnauthorizedError } from '../utils/errors';
import logger from '../utils/logger';
import {
  decodeCursor,
  isValidCursor,
  nextCursor,
  sanitizePageSize,
  CursorError,
  Cursor,
} from '../utils/cursor';

// Module-level singleton used by the standalone route handlers
const stellarService = new StellarService(
  process.env.SOROBAN_RPC_URL || '',
  process.env.LENDING_CONTRACT_ID || ''
);

// ---------------------------------------------------------------------------
// Class-based controller — used for activity / pagination endpoints
// ---------------------------------------------------------------------------

interface ActivityEvent {
  id: string;
  type: 'borrow' | 'repay' | 'deposit' | 'withdraw' | 'liquidate';
  user: string;
  amount: string;
  asset: string;
  ledgerSequence: number;
  eventIndex: number;
  timestamp: string;
  txHash: string;
}

interface PaginatedActivityResponse {
  data: ActivityEvent[];
  pagination: {
    hasNextPage: boolean;
    nextCursor: string | null;
    pageSize: number;
    totalCount: number | null;
  };
}

/**
 * Lending API Controller
 *
 * Handles lending activity endpoints with ledger-sequence-backed
 * pagination cursors for stable ordering guarantees.
 */
export class LendingController {
  private stellarService: StellarService;

  constructor(stellarService: StellarService) {
    this.stellarService = stellarService;
  }

  /**
   * GET /api/lending/activity
   *
   * Returns paginated lending activity ordered by (ledgerSequence ASC, eventIndex ASC).
   * Query params: cursor (opaque base64url), limit (1–100, default 20).
   */
  async getActivity(req: Request, res: Response): Promise<void> {
    try {
      const rawCursor = req.query.cursor as string | undefined;
      let startCursor: Cursor | null = null;

      if (rawCursor !== undefined) {
        if (!isValidCursor(rawCursor)) {
          res.status(400).json({
            error: 'Invalid cursor',
            message: 'The provided cursor is malformed or expired. Request the first page without a cursor.',
            code: 'INVALID_CURSOR',
          });
          return;
        }
        startCursor = decodeCursor(rawCursor);
      }

      const pageSize = sanitizePageSize(req.query.limit);

      const { events } = await this.stellarService.fetchActivityByLedgerRange({
        startLedger: startCursor?.ledgerSequence ?? null,
        startEventIndex: startCursor?.eventIndex ?? null,
        limit: pageSize + 1,
      });

      const hasNextPage = events.length > pageSize;
      const pageEvents = hasNextPage ? events.slice(0, pageSize) : events;

      let nextCursorValue: string | null = null;
      if (hasNextPage && pageEvents.length > 0) {
        const last = pageEvents[pageEvents.length - 1];
        nextCursorValue = nextCursor(last.ledgerSequence, last.eventIndex);
      }

      const response: PaginatedActivityResponse = {
        data: pageEvents,
        pagination: {
          hasNextPage,
          nextCursor: nextCursorValue,
          pageSize: pageEvents.length,
          totalCount: null,
        },
      };

      res.status(200).json(response);
    } catch (error) {
      if (error instanceof CursorError) {
        res.status(400).json({
          error: 'Invalid cursor',
          message: error.message,
          code: 'INVALID_CURSOR',
        });
        return;
      }

      logger.error('Failed to fetch lending activity:', { error });
      res.status(500).json({
        error: 'Internal server error',
        message: 'Failed to fetch lending activity. Please try again.',
        code: 'INTERNAL_ERROR',
      });
    }
  }

  /**
   * GET /api/lending/activity/:userAddress
   *
   * Returns activity for a specific user with cursor pagination.
   */
  async getUserActivity(req: Request, res: Response): Promise<void> {
    try {
      const { userAddress } = req.params;

      if (!userAddress || typeof userAddress !== 'string') {
        res.status(400).json({
          error: 'Invalid user address',
          message: 'User address is required',
          code: 'INVALID_ADDRESS',
        });
        return;
      }

      const rawCursor = req.query.cursor as string | undefined;
      let startCursor: Cursor | null = null;

      if (rawCursor !== undefined) {
        if (!isValidCursor(rawCursor)) {
          res.status(400).json({
            error: 'Invalid cursor',
            message: 'The provided cursor is malformed.',
            code: 'INVALID_CURSOR',
          });
          return;
        }
        startCursor = decodeCursor(rawCursor);
      }

      const pageSize = sanitizePageSize(req.query.limit);

      const { events } = await this.stellarService.fetchUserActivityByLedgerRange({
        userAddress,
        startLedger: startCursor?.ledgerSequence ?? null,
        startEventIndex: startCursor?.eventIndex ?? null,
        limit: pageSize + 1,
      });

      const hasNextPage = events.length > pageSize;
      const pageEvents = hasNextPage ? events.slice(0, pageSize) : events;

      let nextCursorValue: string | null = null;
      if (hasNextPage && pageEvents.length > 0) {
        const last = pageEvents[pageEvents.length - 1];
        nextCursorValue = nextCursor(last.ledgerSequence, last.eventIndex);
      }

      const response: PaginatedActivityResponse = {
        data: pageEvents,
        pagination: {
          hasNextPage,
          nextCursor: nextCursorValue,
          pageSize: pageEvents.length,
          totalCount: null,
        },
      };

      res.status(200).json(response);
    } catch (error) {
      if (error instanceof CursorError) {
        res.status(400).json({
          error: 'Invalid cursor',
          message: error.message,
          code: 'INVALID_CURSOR',
        });
        return;
      }

      logger.error('Failed to fetch user activity:', { error });
      res.status(500).json({
        error: 'Internal server error',
        message: 'Failed to fetch user activity. Please try again.',
        code: 'INTERNAL_ERROR',
      });
    }
  }
}

// ---------------------------------------------------------------------------
// Standalone route handlers — wired in lending.routes.ts
// ---------------------------------------------------------------------------

const lendingController = new LendingController(stellarService);

/**
 * Standalone Express handler for `GET /api/lending/activity`.
 *
 * Exported so the route table can mount the controller without instantiating a
 * fresh service per request.
 */
export const getActivity = lendingController.getActivity.bind(lendingController);

export const deposit = async (req: Request, res: Response, next: NextFunction) => {
  try {
    const { userAddress, assetAddress, amount }: DepositRequest = req.body;

    logger.info('Preparing deposit transaction', { userAddress, amount });

    const prepared = await stellarService.buildDepositTransaction(
      userAddress,
      assetAddress,
      amount
    );

    return res.status(200).json(prepared);
  } catch (error) {
    next(error);
  }
};

export const borrow = async (req: Request, res: Response, next: NextFunction) => {
  try {
    const { userAddress, assetAddress, amount }: BorrowRequest = req.body;

    logger.info('Preparing borrow transaction', { userAddress, amount });

    const prepared = await stellarService.buildBorrowTransaction(
      userAddress,
      assetAddress,
      amount
    );

    return res.status(200).json(prepared);
  } catch (error) {
    next(error);
  }
};

export const repay = async (req: Request, res: Response, next: NextFunction) => {
  try {
    const { userAddress, assetAddress, amount }: RepayRequest = req.body;

    logger.info('Preparing repay transaction', { userAddress, amount });

    const prepared = await stellarService.buildRepayTransaction(
      userAddress,
      assetAddress,
      amount
    );

    return res.status(200).json(prepared);
  } catch (error) {
    next(error);
  }
};

export const withdraw = async (req: Request, res: Response, next: NextFunction) => {
  try {
    const { userAddress, assetAddress, amount }: WithdrawRequest = req.body;

    logger.info('Preparing withdraw transaction', { userAddress, amount });

    const prepared = await stellarService.buildWithdrawTransaction(
      userAddress,
      assetAddress,
      amount
    );

    return res.status(200).json(prepared);
  } catch (error) {
    next(error);
  }
};

export const submit = async (req: AuthRequest, res: Response, next: NextFunction) => {
  try {
    const tokenAddress = req.user?.address;
    if (!tokenAddress) {
      return next(new UnauthorizedError('Access token required'));
    }

    const { xdr: signedXdr }: SubmitTransactionRequest = req.body;

    logger.info('Submitting signed transaction', { userAddress: tokenAddress });

    const result = await stellarService.submitSignedTransaction(signedXdr, tokenAddress);

    if (result.success && result.transactionHash) {
      const monitorResult = await stellarService.monitorTransaction(result.transactionHash);
      return res.status(200).json(monitorResult);
    }

    return res.status(400).json({
      success: false,
      error: result.error || 'Transaction submission failed',
    });
  } catch (error) {
    next(error);
  }
};

export const processHook = async (_req: Request, res: Response) => {
  return res.status(200).json({ success: true });
};
