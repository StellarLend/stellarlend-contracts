import { Request, Response, NextFunction } from 'express';
import { StellarService } from '../services/stellar.service';
import { DepositRequest, BorrowRequest, RepayRequest, WithdrawRequest } from '../types';
import { config } from '../config';
import logger from '../utils/logger';
import {
  decodeCursor,
  isValidCursor,
  nextCursor,
  sanitizePageSize,
  CursorError,
  Cursor,
} from '../utils/cursor';

// Module-level singleton used by the standalone route handlers.
//
// Invariant: this module is loaded by `app.ts` at import time, so the
// constructor arguments must never be empty. They come from the validated
// `config` (which rejects a non-`http` RPC URL at start-up) rather than from
// raw `process.env`, because a blank `SOROBAN_RPC_URL` would otherwise make
// `new rpc.Server('')` throw and take the whole app down before it can serve
// a single request or render a diagnosable error.
const stellarService = new StellarService(
  config.stellar.sorobanRpcUrl,
  config.stellar.contractId
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
    const { userAddress, assetAddress, amount, userSecret }: DepositRequest = req.body;

    logger.info('Processing deposit request', { userAddress, amount });

    const txXdr = await stellarService.buildDepositTransaction(
      userAddress,
      assetAddress,
      amount,
      userSecret
    );

    const result = await stellarService.submitTransaction(txXdr);

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

/**
 * `borrow`, `repay`, and `withdraw` share the deposit flow's shape: build the
 * transaction, submit it, then monitor it. A rejected submission is surfaced as
 * 400 (the caller can fix the request); a thrown failure is forwarded to the
 * central error handler via `next(error)` so a partial failure can never be
 * reported as a success.
 */
const submitOperation = async (
  req: Request,
  res: Response,
  next: NextFunction,
  operation: 'borrow' | 'repay' | 'withdraw',
  build: (
    userAddress: string,
    assetAddress: string | undefined,
    amount: string,
    userSecret: string
  ) => Promise<string>
) => {
  try {
    const { userAddress, assetAddress, amount, userSecret }: BorrowRequest &
      RepayRequest &
      WithdrawRequest = req.body;

    logger.info(`Processing ${operation} request`, { userAddress, amount });

    const txXdr = await build(userAddress, assetAddress, amount, userSecret);

    const result = await stellarService.submitTransaction(txXdr);

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

export const borrow = async (req: Request, res: Response, next: NextFunction) =>
  submitOperation(req, res, next, 'borrow', (userAddress, assetAddress, amount, userSecret) =>
    stellarService.buildBorrowTransaction(userAddress, assetAddress, amount, userSecret)
  );

export const repay = async (req: Request, res: Response, next: NextFunction) =>
  submitOperation(req, res, next, 'repay', (userAddress, assetAddress, amount, userSecret) =>
    stellarService.buildRepayTransaction(userAddress, assetAddress, amount, userSecret)
  );

export const withdraw = async (req: Request, res: Response, next: NextFunction) =>
  submitOperation(req, res, next, 'withdraw', (userAddress, assetAddress, amount, userSecret) =>
    stellarService.buildWithdrawTransaction(userAddress, assetAddress, amount, userSecret)
  );

/**
 * POST /api/lending/hooks/*
 *
 * Reached only after `verifyHookHmac` has authenticated the raw body. The hook
 * payload itself is intentionally opaque: acknowledging it keeps the indexer
 * from retrying an already-delivered event, and acknowledging is idempotent so
 * a duplicate delivery is harmless.
 */
export const processHook = async (req: Request, res: Response, next: NextFunction) => {
  try {
    return res.status(200).json({ success: true, message: 'Hook authenticated' });
  } catch (error) {
    next(error);
  }
};

export const healthCheck = async (req: Request, res: Response, next: NextFunction) => {
  try {
    const services = await stellarService.healthCheck();
    const isHealthy = services.horizon && services.sorobanRpc;

    res.status(isHealthy ? 200 : 503).json({
      status: isHealthy ? 'healthy' : 'unhealthy',
      timestamp: new Date().toISOString(),
      services,
    });
  } catch (error) {
    next(error);
  }
};

export const deepHealthCheck = async (req: Request, res: Response, next: NextFunction) => {
  try {
    const result = await stellarService.pingContract();
    const isHealthy = result.rpc && result.contract;

    res.status(isHealthy ? 200 : 503).json({
      rpc: result.rpc,
      contract: result.contract,
      ledger: result.ledger,
      timestamp: new Date().toISOString(),
    });
  } catch (error) {
    next(error);
  }
};
