import { Request, Response, NextFunction } from 'express';
@import { StellarService } from '../services/stellar.service';
import { DepositRequest, BorrowRequest, RepayRequest, WithdrawRequest } from '../types';
import logger from '../utils/logger';
import {
  decodeCursor,
  isValidCursor,
  getNextCursor,
} from '../utils/cursor';

const DEFAULT_LIMIT = 20;
const MAX_LIMIT = 100;

const stellarService = new StellarService();

export interface ActivityResponse {
  data: Array<{
    id: string;
    type: string;
    ledgerSequence: number;
    eventIndex: number;
    timestamp: string;
    amount: string;
    asset: string;
    account: string;
    txHash: string;
  }>;
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
    nextCursor: string | null;
    hasMore: boolean;
    limit: number;
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

  constructor(stellarService?: StellarService) {
    this.stellarService = stellarService || new StellarService();
  }

  /**
   * GET /api/lending/activity
   *
   * Returns paginated lending activity with cursor-based pagination.
   *
   * Query params:
   * - cursor: base64(ledger_sequence:event_index) — start after this position
   * - limit: items per page (default 20, max 100)
   *
   * The cursor guarantees stable ordering: new events arriving after the cursor
   * won't cause duplicates or gaps in the result set.
   */
  async getActivity(req: Request, res: Response): Promise<void> {
    try {
      const { cursor, limit: limitParam } = req.query;

      // Validate and parse limit
      const limit = this.parseLimit(limitParam);

      // Parse cursor to get starting ledger/event index
      const { fromLedger, fromEventIndex } = this.parseCursor(cursor);

      // Fetch activities from Stellar
      const activities = await this.stellarService.fetchActivities(
        process.env.LENDING_CONTRACT_ID || '',
        {
          fromLedger,
          fromEventIndex,
          limit: limit + 1, // Fetch one extra to determine hasMore
          order: 'desc',
        }
      );

      // Determine if there are more results
      const hasMore = activities.length > limit;
      const results = hasMore ? activities.slice(0, limit) : activities;
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

      // Build response
      const response: ActivityResponse = {
        data: results.map((a) => ({
          id: a.id,
          type: a.type,
          ledgerSequence: a.ledgerSequence,
          eventIndex: a.eventIndex,
          timestamp: a.timestamp.toISOString(),
          amount: a.amount,
          asset: a.asset,
          account: a.account,
          txHash: a.txHash,
        })),
        pagination: {
          nextCursor: hasMore ? getNextCursor(results) || null : null,
          hasMore,
          limit,
          hasNextPage,
          nextCursor: nextCursorValue,
          pageSize: pageEvents.length,
          totalCount: null,
        },
      };

      res.json(response);
    } catch (error) {
      if (error instanceof Error && error.message.includes('Cursor decode failed')) {
        res.status(400).json({
          error: 'Invalid cursor',
          message: error instanceof Error ? error.message : 'Unknown error',
        });
        return;
      }

      logger.error('Failed to fetch lending activity:', { error });
      res.status(500).json({
        error: 'Failed to fetch activity',
        message: error instanceof Error ? error.message : 'Unknown error',
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

      const { cursor, limit: limitParam } = req.query;
      const limit = this.parseLimit(limitParam);
      const { fromLedger, fromEventIndex } = this.parseCursor(cursor);

      const activities = await this.stellarService.fetchUserActivities(
        process.env.LENDING_CONTRACT_ID || '',
        userAddress,
        {
          fromLedger,
          fromEventIndex,
          limit: limit + 1,
          order: 'desc',
        }
      );

      const hasMore = activities.length > limit;
      const results = hasMore ? activities.slice(0, limit) : activities;

      const response: ActivityResponse = {
        data: results.map((a) => ({
          id: a.id,
          type: a.type,
          ledgerSequence: a.ledgerSequence,
          eventIndex: a.eventIndex,
          timestamp: a.timestamp.toISOString(),
          amount: a.amount,
          asset: a.asset,
          account: a.account,
          txHash: a.txHash,
        })),
        pagination: {
          nextCursor: hasMore ? getNextCursor(results) || null : null,
          hasMore,
          limit,
        },
      };

      res.json(response);
    } catch (error) {
      if (error instanceof Error && error.message.includes('Cursor decode failed')) {
        res.status(400).json({
          error: 'Invalid cursor',
          message: error instanceof Error ? error.message : 'Unknown error',
        });
        return;
      }

      console.error('Failed to fetch user activity:', error);
      res.status(500).json({
        error: 'Failed to fetch user activity',
        message: error instanceof Error ? error.message : 'Unknown error',
      });
    }
  }

  private parseLimit(limitParam: unknown): number {
    if (!limitParam) return DEFAULT_LIMIT;

    const parsed = parseInt(limitParam as string, 10);
    if (isNaN(parsed) || parsed <= 0) {
      return DEFAULT_LIMIT;
    }

    return Math.min(parsed, MAX_LIMIT);
  }

  private parseCursor(cursorParam: unknown): { fromLedger?: number; fromEventIndex: number } {
    if (!cursorParam) {
      return { fromEventIndex: 0 };
    }

    const cursor = cursorParam as string;

    if (!isValidCursor(cursor)) {
      throw new Error(`Cursor decode failed: Invalid cursor format`);
    }

    const { ledgerSequence, eventIndex } = decodeCursor(cursor);

    // For pagination, we want to start AFTER the cursor position
    // So we increment the event index within the same ledger
    return {
      fromLedger: ledgerSequence,
      fromEventIndex: eventIndex + 1,
    };
  }
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
