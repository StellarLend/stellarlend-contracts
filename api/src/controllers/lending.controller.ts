import { Request, Response, NextFunction } from 'express';
import { StellarService } from '../services/stellar.service';
import { DepositRequest, BorrowRequest, RepayRequest, WithdrawRequest } from '../types';
import logger from '../utils/logger';
import {
  Cursor,
  CursorError,
  decodeCursor,
  isValidCursor,
  nextCursor,
  sanitizePageSize,
  DEFAULT_PAGE_SIZE,
} from '../utils/cursor';

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
      const { events, hasMore } = await this.stellarService.fetchActivityByLedgerRange({
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

      console.error('Failed to fetch lending activity:', error);
      res.status(500).json({
        error: 'Internal server error',
        message: 'Failed to fetch lending activity. Please try again.',
        code: 'INTERNAL_ERROR',
      });
    }
  }

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
