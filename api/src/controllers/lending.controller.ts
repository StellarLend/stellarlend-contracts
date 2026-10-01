import { Request, Response, NextFunction } from 'express';
@import { StellarService } from '../services/stellar.service';
import { DepositRequest, BorrowRequest, RepayRequest, WithdrawRequest } from '../types';
import logger from '../utils/logger';
import {
  decodeCursor,
  nextCursor,
  sanitizePageSize,
  isValidCursor,
  Cursor,
  CursorError,
  DEFAULT_PAGE_SIZE,
} from '../utils/cursor';

/** Activity event from Stellar ledger */
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

/** Paginated response shape */
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

  constructor(stellarService?: StellarService) {
    this.stellarService = stellarService || new StellarService();
  }

  /**
   * GET /api/lending/activity
   *
   * Returns recent lending activity with ledger-sequence cursor pagination.
   *
   * Query Parameters:
   *   - cursor?: string  - Opaque base64url cursor from a previous page
   *   - limit?: number   - Page size (1-100, default 20)
   *
   * Ordering Guarantee: events are ordered by (ledgerSequence, eventIndex).
   * A cursor captures the exact position of the last returned item, so new
   * events in future ledgers cause neither duplicates nor gaps in earlier
   * cursors.
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
        // Fetch one extra to determine hasNextPage
        limit: pageSize + 1,
      });

      const hasNextPage = events.length > pageSize;
      const pageEvents = hasNextPage ? events.slice(0, pageSize) : events;

      let nextCursorValue: string | null = null;
      if (hasNextPage && pageEvents.length > 0) {
        const lastEvent = pageEvents[pageEvents.length - 1];
        nextCursorValue = nextCursor(lastEvent.ledgerSequence, lastEvent.eventIndex);
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

      logger.error('Failed to fetch lending activity:', error);
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
        const lastEvent = pageEvents[pageEvents.length - 1];
        nextCursorValue = nextCursor(lastEvent.ledgerSequence, lastEvent.eventIndex);
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

      logger.error('Failed to fetch user activity:', error);
      res.status(500).json({
        error: 'Internal server error',
        message: 'Failed to fetch user activity. Please try again.',
        code: 'INTERNAL_ERROR',
      });
    }
  }
}

const stellarService = new StellarService();

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
