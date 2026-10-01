import { Request, Response, NextFunction } from 'express';
import { StellarService } from '../services/stellar.service';
import { DepositRequest, BorrowRequest, RepayRequest, WithdrawRequest } from '../types';
import logger from '../utils/logger';
import {
  encodeCursor,
  decodeCursor,
  isValidCursor,
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
  pagination: {
    nextCursor: string | null;
    hasMore: boolean;
    limit: number;
  };
}

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

      // Parse cursor to get the inclusive start position.
      const { startLedger, startEventIndex } = this.parseCursor(cursor);

      const { events, hasMore } = await this.stellarService.fetchActivityByLedgerRange({
        startLedger,
        startEventIndex,
        limit,
      });

      // Build response
      const response: ActivityResponse = {
        data: events.map((a) => ({
          id: a.id,
          type: a.type,
          ledgerSequence: a.ledgerSequence,
          eventIndex: a.eventIndex,
          timestamp: a.timestamp,
          amount: a.amount,
          asset: a.asset,
          account: a.user,
          txHash: a.txHash,
        })),
        pagination: {
          nextCursor:
            hasMore && events.length > 0
              ? encodeCursor(
                  events[events.length - 1].ledgerSequence,
                  events[events.length - 1].eventIndex,
                )
              : null,
          hasMore,
          limit,
        },
      };

      res.json(response);
    } catch (error) {
      if (error instanceof Error && error.message.includes('Cursor decode failed')) {
        res.status(400).json({
          error: 'Invalid cursor',
          message: error.message,
        });
        return;
      }

      console.error('Failed to fetch lending activity:', error);
      res.status(500).json({
        error: 'Failed to fetch activity',
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

  private parseCursor(cursorParam: unknown): {
    startLedger: number | null;
    startEventIndex: number | null;
  } {
    if (cursorParam === undefined || cursorParam === null || cursorParam === '') {
      return { startLedger: null, startEventIndex: null };
    }

    const cursor = cursorParam as string;

    if (!isValidCursor(cursor)) {
      throw new Error('Cursor decode failed: Invalid cursor format');
    }

    const { ledgerSequence, eventIndex } = decodeCursor(cursor);

    // Resume *after* the cursor position: skip everything at or before it.
    return {
      startLedger: ledgerSequence,
      startEventIndex: eventIndex + 1,
    };
  }
}

const lendingController = new LendingController();

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
