/**
 * Lending Controller Tests
 *
 * Covers cursor pagination, validation, error handling, and ordering
 * guarantees for the activity endpoints.
 */

import { Request, Response } from 'express';
import * as handlers from '../controllers/lending.controller';
import { LendingController } from '../controllers/lending.controller';
import { StellarService } from '../services/stellar.service';
import {
  encodeCursor,
  decodeCursor,
  nextCursor,
  sanitizePageSize,
  isValidCursor,
  CursorError,
  DEFAULT_PAGE_SIZE,
  MAX_PAGE_SIZE,
} from '../utils/cursor';

const mockFetchActivity = jest.fn();
const mockFetchUserActivity = jest.fn();
const mockBuild = {
  deposit: jest.fn(),
  borrow: jest.fn(),
  repay: jest.fn(),
  withdraw: jest.fn(),
};
const mockSubmitTransaction = jest.fn();
const mockMonitorTransaction = jest.fn();
const mockHealthCheck = jest.fn();
const mockPingContract = jest.fn();

// The controller constructs StellarService at module load time, so the mock
// factory must read these lazily rather than capturing the bindings eagerly.
jest.mock('../services/stellar.service', () => ({
  StellarService: jest.fn().mockImplementation(() => ({
    fetchActivityByLedgerRange: (...args: unknown[]) => mockFetchActivity(...args),
    fetchUserActivityByLedgerRange: (...args: unknown[]) => mockFetchUserActivity(...args),
    buildDepositTransaction: (...args: unknown[]) => mockBuild.deposit(...args),
    buildBorrowTransaction: (...args: unknown[]) => mockBuild.borrow(...args),
    buildRepayTransaction: (...args: unknown[]) => mockBuild.repay(...args),
    buildWithdrawTransaction: (...args: unknown[]) => mockBuild.withdraw(...args),
    submitTransaction: (...args: unknown[]) => mockSubmitTransaction(...args),
    monitorTransaction: (...args: unknown[]) => mockMonitorTransaction(...args),
    healthCheck: (...args: unknown[]) => mockHealthCheck(...args),
    pingContract: (...args: unknown[]) => mockPingContract(...args),
  })),
}));

// ============================================================================
// Test Helpers
// ============================================================================

function createMockRequest(
  query: Record<string, unknown> = {},
  params: Record<string, string> = {}
): Partial<Request> {
  return { query, params } as Partial<Request>;
}

function createMockResponse(): Partial<Response> & { json: jest.Mock; status: jest.Mock } {
  const res: any = {};
  res.status = jest.fn().mockReturnValue(res);
  res.json = jest.fn().mockReturnValue(res);
  return res;
}

function createMockEvent(overrides: Partial<any> = {}) {
  return {
    id: 'evt-1',
    type: 'borrow',
    user: 'GABC...',
    amount: '1000000000',
    asset: 'USDC',
    ledgerSequence: 1000,
    eventIndex: 5,
    timestamp: '2026-06-01T00:00:00Z',
    txHash: 'tx-abc',
    ...overrides,
  };
}

// ============================================================================
// Cursor Utility Tests
// ============================================================================

describe('Cursor Utilities', () => {
  describe('encodeCursor / decodeCursor', () => {
    it('should round-trip a valid cursor', () => {
      const cursor = { ledgerSequence: 1000, eventIndex: 5 };
      const encoded = encodeCursor(cursor);
      const decoded = decodeCursor(encoded);
      expect(decoded).toEqual(cursor);
    });

    it('should encode to base64url (no padding)', () => {
      const encoded = encodeCursor({ ledgerSequence: 1, eventIndex: 0 });
      expect(encoded).not.toContain('=');
      expect(encoded).not.toContain('+');
      expect(encoded).not.toContain('/');
    });

    it('should handle boundary values', () => {
      const cursor = { ledgerSequence: 4_294_967_295, eventIndex: 1_000_000 };
      const encoded = encodeCursor(cursor);
      const decoded = decodeCursor(encoded);
      expect(decoded).toEqual(cursor);
    });

    it('should reject negative ledger sequence', () => {
      expect(() => encodeCursor({ ledgerSequence: -1, eventIndex: 0 })).toThrow(CursorError);
    });

    it('should reject negative event index', () => {
      expect(() => encodeCursor({ ledgerSequence: 0, eventIndex: -1 })).toThrow(CursorError);
    });

    it('should reject ledger sequence exceeding u32 max', () => {
      expect(() => encodeCursor({ ledgerSequence: 4_294_967_296, eventIndex: 0 })).toThrow(CursorError);
    });

    it('should reject event index exceeding max', () => {
      expect(() => encodeCursor({ ledgerSequence: 0, eventIndex: 1_000_001 })).toThrow(CursorError);
    });

    it('should reject malformed base64', () => {
      expect(() => decodeCursor('not-valid-base64!!!')).toThrow(CursorError);
    });

    it('should reject cursor without separator', () => {
      const bad = Buffer.from('1000', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow(CursorError);
    });

    it('should reject cursor with non-numeric values', () => {
      const bad = Buffer.from('abc:def', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow(CursorError);
    });

    it('should reject empty string', () => {
      expect(() => decodeCursor('')).toThrow(CursorError);
    });

    it('should reject null/undefined', () => {
      expect(() => decodeCursor(null as any)).toThrow(CursorError);
      expect(() => decodeCursor(undefined as any)).toThrow(CursorError);
    });
  });

  describe('nextCursor', () => {
    it('should increment event index', () => {
      const cursor = nextCursor(1000, 5);
      const decoded = decodeCursor(cursor);
      expect(decoded).toEqual({ ledgerSequence: 1000, eventIndex: 6 });
    });

    it('should handle event index rollover within the same ledger', () => {
      const cursor = nextCursor(1000, 999_999);
      const decoded = decodeCursor(cursor);
      expect(decoded).toEqual({ ledgerSequence: 1000, eventIndex: 1_000_000 });
    });
  });

  describe('sanitizePageSize', () => {
    it('should return default for undefined', () => {
      expect(sanitizePageSize(undefined)).toBe(DEFAULT_PAGE_SIZE);
    });

    it('should return default for null', () => {
      expect(sanitizePageSize(null)).toBe(DEFAULT_PAGE_SIZE);
    });

    it('should parse string numbers', () => {
      expect(sanitizePageSize('50')).toBe(50);
    });

    it('should cap at MAX_PAGE_SIZE', () => {
      expect(sanitizePageSize(200)).toBe(MAX_PAGE_SIZE);
      expect(sanitizePageSize('200')).toBe(MAX_PAGE_SIZE);
    });

    it('should use default for NaN', () => {
      expect(sanitizePageSize('abc')).toBe(DEFAULT_PAGE_SIZE);
    });

    it('should use default for negative', () => {
      expect(sanitizePageSize(-5)).toBe(DEFAULT_PAGE_SIZE);
    });

    it('should use default for zero', () => {
      expect(sanitizePageSize(0)).toBe(DEFAULT_PAGE_SIZE);
    });

    it('should accept valid numbers', () => {
      expect(sanitizePageSize(1)).toBe(1);
      expect(sanitizePageSize(50)).toBe(50);
      expect(sanitizePageSize(MAX_PAGE_SIZE)).toBe(MAX_PAGE_SIZE);
    });
  });

  describe('isValidCursor', () => {
    it('should return true for valid cursor', () => {
      const encoded = encodeCursor({ ledgerSequence: 100, eventIndex: 0 });
      expect(isValidCursor(encoded)).toBe(true);
    });

    it('should return false for invalid cursor', () => {
      expect(isValidCursor('invalid')).toBe(false);
    });

    it('should return false for non-string', () => {
      expect(isValidCursor(123)).toBe(false);
      expect(isValidCursor(null)).toBe(false);
      expect(isValidCursor(undefined)).toBe(false);
    });
  });
});

// ============================================================================
// Controller Tests
// ============================================================================

describe('LendingController', () => {
  let controller: LendingController;

  beforeEach(() => {
    jest.clearAllMocks();
    controller = new LendingController(new StellarService());
  });

  describe('GET /api/lending/activity', () => {
    it('should return first page without cursor', async () => {
      // The service is asked for limit+1 items so the controller can detect
      // that another page exists; only `limit` items are returned to the client.
      const page = [
        createMockEvent({ ledgerSequence: 1000, eventIndex: 0 }),
        createMockEvent({ ledgerSequence: 1000, eventIndex: 1 }),
      ];
      const lookahead = createMockEvent({ ledgerSequence: 1000, eventIndex: 2 });
      mockFetchActivity.mockResolvedValue({ events: [...page, lookahead], hasMore: true });

      const req = createMockRequest({ limit: '2' });
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      expect(res.status).toHaveBeenCalledWith(200);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({
          data: page,
          pagination: expect.objectContaining({
            hasNextPage: true,
            nextCursor: expect.any(String),
            pageSize: 2,
          }),
        })
      );

      // The next cursor must encode the position just past the last returned item.
      const responseData = (res.json as jest.Mock).mock.calls[0][0];
      const decoded = decodeCursor(responseData.pagination.nextCursor);
      expect(decoded).toEqual({ ledgerSequence: 1000, eventIndex: 2 });
    });

    it('should paginate with cursor', async () => {
      const cursor = encodeCursor({ ledgerSequence: 1000, eventIndex: 2 });
      const events = [
        createMockEvent({ ledgerSequence: 1000, eventIndex: 2 }),
        createMockEvent({ ledgerSequence: 1001, eventIndex: 0 }),
      ];
      mockFetchActivity.mockResolvedValue({ events, hasMore: false });

      const req = createMockRequest({ cursor, limit: '2' });
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      expect(mockFetchActivity).toHaveBeenCalledWith(
        expect.objectContaining({
          startLedger: 1000,
          startEventIndex: 2,
          limit: 3, // pageSize + 1
        })
      );

      expect(res.status).toHaveBeenCalledWith(200);
      const responseData = (res.json as jest.Mock).mock.calls[0][0];
      expect(responseData.pagination.hasNextPage).toBe(false);
      expect(responseData.pagination.nextCursor).toBeNull();
    });

    it('should return 400 for invalid cursor', async () => {
      const req = createMockRequest({ cursor: 'invalid-cursor' });
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      expect(res.status).toHaveBeenCalledWith(400);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({
          error: 'Invalid cursor',
          code: 'INVALID_CURSOR',
        })
      );
    });

    it('should return 400 for malformed base64 cursor', async () => {
      const req = createMockRequest({ cursor: '!!!not-base64!!!' });
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      expect(res.status).toHaveBeenCalledWith(400);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({ code: 'INVALID_CURSOR' })
      );
    });

    it('should return 400 for an out-of-range cursor', async () => {
      const bad = Buffer.from('99999999999:0', 'utf-8').toString('base64url');
      const req = createMockRequest({ cursor: bad });
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      expect(res.status).toHaveBeenCalledWith(400);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({ code: 'INVALID_CURSOR' })
      );
    });

    it('should use default page size when limit omitted', async () => {
      mockFetchActivity.mockResolvedValue({ events: [], hasMore: false });

      const req = createMockRequest({});
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      expect(mockFetchActivity).toHaveBeenCalledWith(
        expect.objectContaining({
          limit: DEFAULT_PAGE_SIZE + 1,
          startLedger: null,
          startEventIndex: null,
        })
      );
    });

    it('should cap page size at MAX_PAGE_SIZE', async () => {
      mockFetchActivity.mockResolvedValue({ events: [], hasMore: false });

      const req = createMockRequest({ limit: '500' });
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      expect(mockFetchActivity).toHaveBeenCalledWith(
        expect.objectContaining({
          limit: MAX_PAGE_SIZE + 1,
        })
      );
    });

    it('should fall back to the default page size for a hostile limit', async () => {
      mockFetchActivity.mockResolvedValue({ events: [], hasMore: false });

      const req = createMockRequest({ limit: 'not-a-number' });
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      expect(mockFetchActivity).toHaveBeenCalledWith(
        expect.objectContaining({ limit: DEFAULT_PAGE_SIZE + 1 })
      );
    });

    it('should handle empty result set', async () => {
      mockFetchActivity.mockResolvedValue({ events: [], hasMore: false });

      const req = createMockRequest({});
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      expect(res.status).toHaveBeenCalledWith(200);
      const data = (res.json as jest.Mock).mock.calls[0][0];
      expect(data.data).toEqual([]);
      expect(data.pagination.hasNextPage).toBe(false);
      expect(data.pagination.nextCursor).toBeNull();
    });

    it('should handle service errors gracefully', async () => {
      mockFetchActivity.mockRejectedValue(new Error('RPC timeout'));

      const req = createMockRequest({});
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      expect(res.status).toHaveBeenCalledWith(500);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({
          error: 'Internal server error',
          code: 'INTERNAL_ERROR',
        })
      );
    });

    it('should not leak internal error details to the client', async () => {
      mockFetchActivity.mockRejectedValue(new Error('postgres://user:hunter2@internal'));

      const req = createMockRequest({});
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      const data = (res.json as jest.Mock).mock.calls[0][0];
      expect(JSON.stringify(data)).not.toContain('hunter2');
    });

    it('should not miss or duplicate entries across pages', async () => {
      const allEvents = [
        createMockEvent({ id: 'evt-1', ledgerSequence: 100, eventIndex: 0 }),
        createMockEvent({ id: 'evt-2', ledgerSequence: 100, eventIndex: 1 }),
        createMockEvent({ id: 'evt-3', ledgerSequence: 100, eventIndex: 2 }),
        createMockEvent({ id: 'evt-4', ledgerSequence: 101, eventIndex: 0 }),
        createMockEvent({ id: 'evt-5', ledgerSequence: 101, eventIndex: 1 }),
      ];

      // Page 1: service returns limit+1 (3) items, client receives 2.
      mockFetchActivity.mockResolvedValueOnce({
        events: allEvents.slice(0, 3),
        hasMore: true,
      });

      const req1 = createMockRequest({ limit: '2' });
      const res1 = createMockResponse();
      await controller.getActivity(req1 as Request, res1 as Response);

      const data1 = (res1.json as jest.Mock).mock.calls[0][0];
      expect(data1.data).toHaveLength(2);
      expect(data1.data[0].id).toBe('evt-1');
      expect(data1.data[1].id).toBe('evt-2');
      expect(data1.pagination.hasNextPage).toBe(true);

      // Page 2: the service is given the cursor position and returns the
      // remaining 3 events; the client receives the first 2 of them.
      const cursor = data1.pagination.nextCursor;
      expect(decodeCursor(cursor)).toEqual({ ledgerSequence: 100, eventIndex: 2 });

      mockFetchActivity.mockResolvedValueOnce({
        events: allEvents.slice(2),
        hasMore: true,
      });

      const req2 = createMockRequest({ cursor, limit: '2' });
      const res2 = createMockResponse();
      await controller.getActivity(req2 as Request, res2 as Response);

      expect(mockFetchActivity).toHaveBeenLastCalledWith(
        expect.objectContaining({
          startLedger: 100,
          startEventIndex: 2,
        })
      );

      const data2 = (res2.json as jest.Mock).mock.calls[0][0];
      expect(data2.data).toHaveLength(2);
      expect(data2.data[0].id).toBe('evt-3');
      expect(data2.data[1].id).toBe('evt-4');
      expect(data2.pagination.hasNextPage).toBe(true);

      // Page 3: the final event, with no lookahead item to signal another page.
      const cursor2 = data2.pagination.nextCursor;
      mockFetchActivity.mockResolvedValueOnce({
        events: allEvents.slice(4),
        hasMore: false,
      });

      const req3 = createMockRequest({ cursor: cursor2, limit: '2' });
      const res3 = createMockResponse();
      await controller.getActivity(req3 as Request, res3 as Response);

      const data3 = (res3.json as jest.Mock).mock.calls[0][0];
      expect(data3.data).toHaveLength(1);
      expect(data3.data[0].id).toBe('evt-5');
      expect(data3.pagination.hasNextPage).toBe(false);
      expect(data3.pagination.nextCursor).toBeNull();

      // Across all pages every event appears exactly once: no gaps, no dupes.
      const seen = [...data1.data, ...data2.data, ...data3.data].map((e: any) => e.id);
      expect(seen).toEqual(['evt-1', 'evt-2', 'evt-3', 'evt-4', 'evt-5']);
      expect(new Set(seen).size).toBe(seen.length);
    });
  });

  describe('GET /api/lending/activity/:userAddress', () => {
    it('should return user-specific activity', async () => {
      const userAddress = 'GABC123...';
      const events = [createMockEvent({ user: userAddress, ledgerSequence: 100, eventIndex: 0 })];
      mockFetchUserActivity.mockResolvedValue({ events, hasMore: false });

      const req = createMockRequest({ limit: '10' }, { userAddress });
      const res = createMockResponse();

      await controller.getUserActivity(req as Request, res as Response);

      expect(res.status).toHaveBeenCalledWith(200);
      expect(mockFetchUserActivity).toHaveBeenCalledWith(
        expect.objectContaining({
          userAddress,
          startLedger: null,
          startEventIndex: null,
          limit: 11,
        })
      );
    });

    it('should return 400 for missing user address', async () => {
      const req = createMockRequest({}, {});
      const res = createMockResponse();

      await controller.getUserActivity(req as Request, res as Response);

      expect(res.status).toHaveBeenCalledWith(400);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({
          error: 'Invalid user address',
          code: 'INVALID_ADDRESS',
        })
      );
      // The service must not be reached with an unvalidated address.
      expect(mockFetchUserActivity).not.toHaveBeenCalled();
    });

    it('should paginate user activity with cursor', async () => {
      const userAddress = 'GABC123...';
      const cursor = encodeCursor({ ledgerSequence: 100, eventIndex: 5 });
      const events = [createMockEvent({ user: userAddress, ledgerSequence: 100, eventIndex: 5 })];
      mockFetchUserActivity.mockResolvedValue({ events, hasMore: false });

      const req = createMockRequest({ cursor }, { userAddress });
      const res = createMockResponse();

      await controller.getUserActivity(req as Request, res as Response);

      expect(mockFetchUserActivity).toHaveBeenCalledWith(
        expect.objectContaining({
          userAddress,
          startLedger: 100,
          startEventIndex: 5,
        })
      );
    });

    it('should return 400 for an invalid cursor on user activity', async () => {
      const req = createMockRequest({ cursor: 'garbage' }, { userAddress: 'GABC' });
      const res = createMockResponse();

      await controller.getUserActivity(req as Request, res as Response);

      expect(res.status).toHaveBeenCalledWith(400);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({ code: 'INVALID_CURSOR' })
      );
      expect(mockFetchUserActivity).not.toHaveBeenCalled();
    });

    it('should handle service errors gracefully', async () => {
      mockFetchUserActivity.mockRejectedValue(new Error('RPC timeout'));

      const req = createMockRequest({}, { userAddress: 'GABC' });
      const res = createMockResponse();

      await controller.getUserActivity(req as Request, res as Response);

      expect(res.status).toHaveBeenCalledWith(500);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({ code: 'INTERNAL_ERROR' })
      );
    });
  });
});

// ============================================================================
// Ordering Guarantee Tests
// ============================================================================

describe('Activity Ordering Guarantees', () => {
  it('should maintain stable ordering across ledger boundaries', () => {
    const cursors = [
      { ledgerSequence: 100, eventIndex: 5 },
      { ledgerSequence: 100, eventIndex: 10 },
      { ledgerSequence: 101, eventIndex: 0 },
      { ledgerSequence: 101, eventIndex: 3 },
      { ledgerSequence: 102, eventIndex: 1 },
    ];

    const encoded = cursors.map(encodeCursor);
    const decoded = encoded.map(decodeCursor);

    for (let i = 0; i < decoded.length - 1; i++) {
      const a = decoded[i];
      const b = decoded[i + 1];
      const aKey = a.ledgerSequence * 1_000_000 + a.eventIndex;
      const bKey = b.ledgerSequence * 1_000_000 + b.eventIndex;
      expect(aKey).toBeLessThan(bKey);
    }
  });

  it('should handle cursor at ledger boundary correctly', () => {
    const endOfLedger = { ledgerSequence: 100, eventIndex: 999 };
    const next = decodeCursor(nextCursor(endOfLedger.ledgerSequence, endOfLedger.eventIndex));

    expect(next.ledgerSequence).toBe(100);
    expect(next.eventIndex).toBe(1000);
  });
});

// ============================================================================
// Standalone Route Handlers
// ============================================================================

describe('Standalone lending handlers', () => {
  const VALID_ADDRESS = 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK';
  const body = {
    userAddress: VALID_ADDRESS,
    assetAddress: undefined,
    amount: '1000000',
    userSecret: 'SAOS4OGIK6HD4QGR3DVRRDSR4FUBH73FCZGRZ7M53LRN67UQE5JDNS4I',
  };

  let next: jest.Mock;

  beforeEach(() => {
    jest.clearAllMocks();
    next = jest.fn();
    Object.values(mockBuild).forEach(fn => fn.mockResolvedValue('tx_xdr'));
    mockSubmitTransaction.mockResolvedValue({ success: false, status: 'failed' });
    mockMonitorTransaction.mockResolvedValue({ success: true, status: 'success' });
  });

  const operations = [
    { name: 'deposit', handler: handlers.deposit, build: mockBuild.deposit },
    { name: 'borrow', handler: handlers.borrow, build: mockBuild.borrow },
    { name: 'repay', handler: handlers.repay, build: mockBuild.repay },
    { name: 'withdraw', handler: handlers.withdraw, build: mockBuild.withdraw },
  ] as const;

  describe.each(operations)('$name', ({ handler, build }) => {
    it('returns 200 with the monitor result when submission succeeds', async () => {
      mockSubmitTransaction.mockResolvedValue({
        success: true,
        status: 'pending',
        transactionHash: 'hash-1',
      });
      const res = createMockResponse();

      await handler({ body } as Partial<Request>, res as Response, next);

      expect(build).toHaveBeenCalledWith(VALID_ADDRESS, undefined, '1000000', body.userSecret);
      expect(mockMonitorTransaction).toHaveBeenCalledWith('hash-1');
      expect(res.status).toHaveBeenCalledWith(200);
    });

    it('returns 400 when submission fails', async () => {
      mockSubmitTransaction.mockResolvedValue({ success: false, status: 'failed', error: 'nope' });
      const res = createMockResponse();

      await handler({ body } as Partial<Request>, res as Response, next);

      expect(res.status).toHaveBeenCalledWith(400);
      expect(mockMonitorTransaction).not.toHaveBeenCalled();
    });

    it('forwards a build failure to the error handler', async () => {
      build.mockRejectedValue(new Error('build failed'));
      const res = createMockResponse();

      await handler({ body } as Partial<Request>, res as Response, next);

      expect(next).toHaveBeenCalledWith(expect.any(Error));
      expect(res.status).not.toHaveBeenCalled();
    });
  });

  describe('processHook', () => {
    it('acknowledges an authenticated hook', async () => {
      const res = createMockResponse();

      await handlers.processHook({} as Partial<Request>, res as Response, next);

      expect(res.status).toHaveBeenCalledWith(200);
      expect(res.json).toHaveBeenCalledWith({ success: true, message: 'Hook authenticated' });
    });

    it('forwards a serialization failure to the error handler', async () => {
      const res = createMockResponse();
      res.json.mockImplementation(() => {
        throw new Error('serialization failed');
      });

      await handlers.processHook({} as Partial<Request>, res as Response, next);

      expect(next).toHaveBeenCalledWith(expect.any(Error));
    });
  });

  describe('healthCheck', () => {
    it('returns 200 when both dependencies are healthy', async () => {
      mockHealthCheck.mockResolvedValue({ horizon: true, sorobanRpc: true });
      const res = createMockResponse();

      await handlers.healthCheck({} as Partial<Request>, res as Response, next);

      expect(res.status).toHaveBeenCalledWith(200);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({ status: 'healthy' })
      );
    });

    it('returns 503 when a dependency is down', async () => {
      mockHealthCheck.mockResolvedValue({ horizon: true, sorobanRpc: false });
      const res = createMockResponse();

      await handlers.healthCheck({} as Partial<Request>, res as Response, next);

      expect(res.status).toHaveBeenCalledWith(503);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({ status: 'unhealthy' })
      );
    });

    it('forwards a probe failure to the error handler', async () => {
      mockHealthCheck.mockRejectedValue(new Error('probe exploded'));
      const res = createMockResponse();

      await handlers.healthCheck({} as Partial<Request>, res as Response, next);

      expect(next).toHaveBeenCalledWith(expect.any(Error));
      expect(res.status).not.toHaveBeenCalled();
    });
  });

  describe('deepHealthCheck', () => {
    it('returns 200 when the contract is reachable', async () => {
      mockPingContract.mockResolvedValue({ rpc: true, contract: true, ledger: 42 });
      const res = createMockResponse();

      await handlers.deepHealthCheck({} as Partial<Request>, res as Response, next);

      expect(res.status).toHaveBeenCalledWith(200);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({ rpc: true, contract: true, ledger: 42 })
      );
    });

    it('returns 503 when the contract is unreachable', async () => {
      mockPingContract.mockResolvedValue({ rpc: true, contract: false, ledger: null });
      const res = createMockResponse();

      await handlers.deepHealthCheck({} as Partial<Request>, res as Response, next);

      expect(res.status).toHaveBeenCalledWith(503);
    });

    it('forwards a probe failure to the error handler', async () => {
      mockPingContract.mockRejectedValue(new Error('probe exploded'));
      const res = createMockResponse();

      await handlers.deepHealthCheck({} as Partial<Request>, res as Response, next);

      expect(next).toHaveBeenCalledWith(expect.any(Error));
    });
  });
});

describe('LendingController cursor error handling', () => {
  let controller: LendingController;

  beforeEach(() => {
    jest.clearAllMocks();
    mockFetchActivity.mockResolvedValue({ events: [] });
    mockFetchUserActivity.mockResolvedValue({ events: [] });
    controller = new LendingController(new StellarService('https://rpc.test', 'contract'));
  });

  it('maps a cursor decoding failure to 400 on the activity feed', async () => {
    // A cursor that passes the shape check but decodes out of range.
    const outOfRange = Buffer.from('99999999999:1').toString('base64url');
    const res = createMockResponse();

    await controller.getActivity(
      createMockRequest({ cursor: outOfRange }) as Request,
      res as Response
    );

    expect(res.status).toHaveBeenCalledWith(400);
    expect(res.json).toHaveBeenCalledWith(
      expect.objectContaining({ code: 'INVALID_CURSOR' })
    );
  });

  it('maps a cursor decoding failure to 400 on the user feed', async () => {
    const outOfRange = Buffer.from('99999999999:1').toString('base64url');
    const res = createMockResponse();

    await controller.getUserActivity(
      { query: { cursor: outOfRange }, params: { userAddress: 'GABC' } } as unknown as Request,
      res as Response
    );

    expect(res.status).toHaveBeenCalledWith(400);
    expect(res.json).toHaveBeenCalledWith(
      expect.objectContaining({ code: 'INVALID_CURSOR' })
    );
  });

  it('issues a next cursor for a full page of user activity', async () => {
    mockFetchUserActivity.mockResolvedValue({
      events: [
        createMockEvent({ id: 'a', ledgerSequence: 100, eventIndex: 0 }),
        createMockEvent({ id: 'b', ledgerSequence: 100, eventIndex: 1 }),
      ],
    });
    const res = createMockResponse();

    await controller.getUserActivity(
      { query: { limit: '1' }, params: { userAddress: 'GABC' } } as unknown as Request,
      res as Response
    );

    expect(res.status).toHaveBeenCalledWith(200);
    expect(res.json).toHaveBeenCalledWith(
      expect.objectContaining({
        pagination: expect.objectContaining({
          hasNextPage: true,
          pageSize: 1,
          nextCursor: expect.any(String),
        }),
      })
    );
  });
});
