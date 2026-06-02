import { Request, Response, NextFunction } from 'express';
import { StellarService } from '../services/stellar.service';
import { DepositRequest, BorrowRequest, RepayRequest, WithdrawRequest } from '../types';
import logger from '../utils/logger';
/**
 * Lending API Controller
 *
 * Handles lending activity endpoints with ledger-sequence-backed
 * pagination cursors for stable ordering guarantees.
 *
 * @see docs/ACTIVITY_ORDERING_GUARANTEES.md
 */
import {
  encodeCursor,
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
    totalCount: number | null; // null if unknown/counting disabled
  };
}

export class LendingController {
  private stellarService: StellarService;

  constructor(stellarService: StellarService) {
    this.stellarService = stellarService;
  }


  /**
   * GET /api/lending/activity
   *
   * Returns recent lending activity with ledger-sequence cursor pagination.
   *
   * Query Parameters:
   *   - cursor?: string   - Base64-encoded cursor from previous page (opaque)
   *   - limit?: number    - Page size (1-100, default 20)
   *
   * Response:
   *   {
   *     "data": [...ActivityEvent],
   *     "pagination": {
   *       "hasNextPage": boolean,
   *       "nextCursor": string | null,
   *       "pageSize": number,
   *       "totalCount": number | null
   *     }
   *   }
   *
   * Cursor Format: base64(ledger_sequence:event_index)
   * - ledger_sequence: The Stellar ledger sequence number (monotonically increasing)
   * - event_index: Position within the ledger (0-based, stable within a ledger)
   *
   * Ordering Guarantee:
   * - Events are ordered by (ledgerSequence ASC, eventIndex ASC)
   * - The cursor captures the exact position of the last returned item
   * - New events in future ledgers do not affect pagination of past cursors
   * - No duplicate or missed entries when new events arrive between calls
   */
  async getActivity(req: Request, res: Response): Promise<void> {
    try {
      // Parse and validate cursor
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

      // Parse and validate page size
      const pageSize = sanitizePageSize(req.query.limit);

      // Fetch events from Stellar service
      const { events, hasMore } = await this.stellarService.fetchActivityByLedgerRange({
        startLedger: startCursor?.ledgerSequence ?? null,
        startEventIndex: startCursor?.eventIndex ?? null,
        limit: pageSize + 1, // Fetch one extra to determine hasNextPage
      });

      // Determine pagination state
      const hasNextPage = events.length > pageSize;
      const pageEvents = hasNextPage ? events.slice(0, pageSize) : events;

      // Generate next cursor from the last item
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
          totalCount: null, // Counting all events is expensive; omit for performance
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

      console.error('Failed to fetch lending activity:', error);
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

      // Validate user address
      if (!userAddress || typeof userAddress !== 'string') {
        res.status(400).json({
          error: 'Invalid user address',
          message: 'User address is required',
          code: 'INVALID_ADDRESS',
        });
        return;
      }

      // Parse cursor and limit
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

      // Fetch user-specific events
      const { events, hasMore } = await this.stellarService.fetchUserActivityByLedgerRange({
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

      console.error('Failed to fetch user activity:', error);
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

    return res.status(400).json(result);
  } catch (error) {
    next(error);
  }
};

export const borrow = async (req: Request, res: Response, next: NextFunction) => {
  try {
    const { userAddress, assetAddress, amount, userSecret }: BorrowRequest = req.body;

    logger.info('Processing borrow request', { userAddress, amount });

    const txXdr = await stellarService.buildBorrowTransaction(
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

    return res.status(400).json(result);
  } catch (error) {
    next(error);
  }
};

export const repay = async (req: Request, res: Response, next: NextFunction) => {
  try {
    const { userAddress, assetAddress, amount, userSecret }: RepayRequest = req.body;

    logger.info('Processing repay request', { userAddress, amount });

    const txXdr = await stellarService.buildRepayTransaction(
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

    return res.status(400).json(result);
  } catch (error) {
    next(error);
  }
};

export const withdraw = async (req: Request, res: Response, next: NextFunction) => {
  try {
    const { userAddress, assetAddress, amount, userSecret }: WithdrawRequest = req.body;

    logger.info('Processing withdraw request', { userAddress, amount });

    const txXdr = await stellarService.buildWithdrawTransaction(
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

    return res.status(400).json(result);
  } catch (error) {
    next(error);
  }
};

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
