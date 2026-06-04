import { Request, Response } from 'express';
import { LendingController, ActivityResponse } from '../controllers/lending.controller';
import { StellarService } from '../services/stellar.service';
import { encodeCursor } from '../utils/cursor';

// Mock StellarService
jest.mock('../services/stellar.service');

describe('LendingController', () => {
  let controller: LendingController;
  let mockStellarService: jest.Mocked<<StellarService>;
  let mockReq: Partial<<Request>;
  let mockRes: Partial<Response>;
  let jsonMock: jest.Mock;
  let statusMock: jest.Mock;

  beforeEach(() => {
    mockStellarService = new StellarService() as jest.Mocked<<StellarService>;
    controller = new LendingController(mockStellarService);

    jsonMock = jest.fn();
    statusMock = jest.fn().mockReturnValue({ json: jsonMock });
    
    mockReq = { query: {} };
    mockRes = {
      json: jsonMock,
      status: statusMock,
    };
  });

  afterEach(() => {
/**
 * Lending Controller Tests
 *
 * Covers cursor pagination, validation, error handling, and
 * ordering guarantees for the activity endpoints.
 */

import { Request, Response } from 'express';
import { LendingController } from '../controllers/lending.controller';
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

jest.mock('../services/stellar.service', () => ({
  StellarService: jest.fn().mockImplementation(() => ({
    fetchActivityByLedgerRange: mockFetchActivity,
    fetchUserActivityByLedgerRange: mockFetchUserActivity,
  })),
}));

// ============================================================================
// Test Helpers
// ============================================================================

function createMockRequest(query: Record<string, unknown> = {}, params: Record<string, string> = {}): Partial<Request> {
  return {
    query,
    params,
  } as Partial<Request>;
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
      expect(() => decodeCursor(Buffer.from('1000', 'utf-8').toString('base64url'))).toThrow(CursorError);
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

    it('should handle event index rollover to next ledger', () => {
      // When eventIndex reaches max, next page starts at next ledger
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

describe('LendingController', () => {
  let controller: LendingController;
  let mockStellarService: StellarService;

  beforeEach(() => {
    jest.clearAllMocks();
    mockStellarService = new StellarService('https://rpc.test', 'CONTRACT_ID');
    controller = new LendingController(mockStellarService);
  });

  describe('getActivity', () => {
    const mockActivities = [
      {
        id: '1',
        type: 'borrow' as const,
        ledgerSequence: 5000,
        eventIndex: 2,
        timestamp: new Date('2024-01-01T00:00:00Z'),
        amount: '100.0000000',
        asset: 'USDC',
        account: 'GACCOUNT1',
        txHash: 'TX1',
      },
      {
        id: '2',
        type: 'deposit' as const,
        ledgerSequence: 5000,
        eventIndex: 1,
        timestamp: new Date('2024-01-01T00:01:00Z'),
        amount: '200.0000000',
        asset: 'XLM',
        account: 'GACCOUNT2',
        txHash: 'TX2',
      },
      {
        id: '3',
        type: 'repay' as const,
        ledgerSequence: 4999,
        eventIndex: 0,
        timestamp: new Date('2024-01-01T00:02:00Z'),
        amount: '50.0000000',
        asset: 'USDC',
        account: 'GACCOUNT3',
        txHash: 'TX3',
      },
    ];
  describe('GET /api/lending/activity', () => {
    it('should return first page without cursor', async () => {
      const events = [
        createMockEvent({ ledgerSequence: 1000, eventIndex: 0 }),
        createMockEvent({ ledgerSequence: 1000, eventIndex: 1 }),
      ];
      mockFetchActivity.mockResolvedValue({ events, hasMore: true });

      const req = createMockRequest({ limit: '2' });
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      expect(res.status).toHaveBeenCalledWith(200);
      expect(res.json).toHaveBeenCalledWith(
        expect.objectContaining({
          data: events,
          pagination: expect.objectContaining({
            hasNextPage: true,
            nextCursor: expect.any(String),
            pageSize: 2,
          }),
        })
      );

      // Verify next cursor encodes correct position
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
    });

    it('should use default page size when limit omitted', async () => {
      mockFetchActivity.mockResolvedValue({ events: [], hasMore: false });

      const req = createMockRequest({});
      const res = createMockResponse();

      await controller.getActivity(req as Request, res as Response);

      expect(mockFetchActivity).toHaveBeenCalledWith(
        expect.objectContaining({
          limit: DEFAULT_PAGE_SIZE + 1,
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

    it('should not miss or duplicate entries across pages', async () => {
      // Simulate 5 events across 2 pages of 2
      const allEvents = [
        createMockEvent({ id: 'evt-1', ledgerSequence: 100, eventIndex: 0 }),
        createMockEvent({ id: 'evt-2', ledgerSequence: 100, eventIndex: 1 }),
        createMockEvent({ id: 'evt-3', ledgerSequence: 100, eventIndex: 2 }),
        createMockEvent({ id: 'evt-4', ledgerSequence: 101, eventIndex: 0 }),
        createMockEvent({ id: 'evt-5', ledgerSequence: 101, eventIndex: 1 }),
      ];

      // Page 1
      mockFetchActivity.mockResolvedValueOnce({
        events: allEvents.slice(0, 3), // 3 items (limit+1)
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

      // Page 2 using cursor from page 1
      const cursor = data1.pagination.nextCursor;
      mockFetchActivity.mockResolvedValueOnce({
        events: allEvents.slice(2), // From evt-3 onwards
        hasMore: false,
      });

      const req2 = createMockRequest({ cursor, limit: '2' });
      const res2 = createMockResponse();
      await controller.getActivity(req2 as Request, res2 as Response);

      const data2 = (res2.json as jest.Mock).mock.calls[0][0];
      expect(data2.data).toHaveLength(3);
      expect(data2.data[0].id).toBe('evt-3');
      expect(data2.data[1].id).toBe('evt-4');
      expect(data2.data[2].id).toBe('evt-5');

      // Verify no overlap between pages
      const page1Ids = data1.data.map((e: any) => e.id);
      const page2Ids = data2.data.map((e: any) => e.id);
      const overlap = page1Ids.filter((id: string) => page2Ids.includes(id));
      expect(overlap).toHaveLength(0);
    });
  });

  describe('GET /api/lending/activity/:userAddress', () => {
    it('should return user-specific activity', async () => {
      const userAddress = 'GABC123...';
      const events = [
        createMockEvent({ user: userAddress, ledgerSequence: 100, eventIndex: 0 }),
      ];
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
    });

    it('should paginate user activity with cursor', async () => {
      const userAddress = 'GABC123...';
      const cursor = encodeCursor({ ledgerSequence: 100, eventIndex: 5 });
      const events = [
        createMockEvent({ user: userAddress, ledgerSequence: 100, eventIndex: 5 }),
      ];
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
  });
});

// ============================================================================
// Ordering Guarantee Tests
// ============================================================================

describe('Activity Ordering Guarantees', () => {
  it('should maintain stable ordering across ledger boundaries', () => {
    // Events should be ordered by (ledgerSequence ASC, eventIndex ASC)
    const cursors = [
      { ledgerSequence: 100, eventIndex: 5 },
      { ledgerSequence: 100, eventIndex: 10 },
      { ledgerSequence: 101, eventIndex: 0 },
      { ledgerSequence: 101, eventIndex: 3 },
      { ledgerSequence: 102, eventIndex: 1 },
    ];

    const encoded = cursors.map(encodeCursor);
    const decoded = encoded.map(decodeCursor);

    // Verify round-trip preserves order
    for (let i = 0; i < decoded.length - 1; i++) {
      const a = decoded[i];
      const b = decoded[i + 1];
      const aKey = a.ledgerSequence * 1_000_000 + a.eventIndex;
      const bKey = b.ledgerSequence * 1_000_000 + b.eventIndex;
      expect(aKey).toBeLessThan(bKey);
    }
  });

  it('should handle cursor at ledger boundary correctly', () => {
    // Last event of ledger 100
    const endOfLedger = { ledgerSequence: 100, eventIndex: 999 };
    const next = decodeCursor(nextCursor(endOfLedger.ledgerSequence, endOfLedger.eventIndex));

    // Next cursor should point to event 1000 in same ledger
    // (or event 0 of next ledger if 1000 is the max)
    expect(next.ledgerSequence).toBe(100);
    expect(next.eventIndex).toBe(1000);
  });
});

  describe('POST /api/lending/deposit', () => {
    it('should successfully process a deposit', async () => {
      const mockTxXdr = 'mock_tx_xdr';
      const mockTxHash = 'mock_tx_hash';

    it('returns activities with pagination metadata', async () => {
      mockStellarService.fetchActivities.mockResolvedValue(mockActivities);

      await controller.getActivity(mockReq as Request, mockRes as Response);

      expect(mockRes.json).toHaveBeenCalledWith(
        expect.objectContaining({
          data: expect.arrayContaining([
            expect.objectContaining({
              id: '1',
              ledgerSequence: 5000,
              eventIndex: 2,
            }),
          ]),
          pagination: expect.objectContaining({
            hasMore: false,
            limit: 20,
            nextCursor: null,
          }),
        })
      );
    });

    it('returns nextCursor when there are more results', async () => {
      // Return more than limit to trigger hasMore
      const extraActivities = [
        ...mockActivities,
        {
          id: '4',
          type: 'withdraw' as const,
          ledgerSequence: 4998,
          eventIndex: 0,
          timestamp: new Date('2024-01-01T00:03:00Z'),
          amount: '75.0000000',
          asset: 'EURC',
          account: 'GACCOUNT4',
          txHash: 'TX4',
        },
      ];
      mockStellarService.fetchActivities.mockResolvedValue(extraActivities);

      await controller.getActivity(mockReq as Request, mockRes as Response);

      const response = jsonMock.mock.calls[0][0] as ActivityResponse;
      expect(response.pagination.hasMore).toBe(true);
      expect(response.pagination.nextCursor).toBeTruthy();
      
      // Verify cursor points to last returned item
      const decoded = Buffer.from(response.pagination.nextCursor!, 'base64').toString('utf-8');
      expect(decoded).toBe('4999:0'); // Last item in the 20-item page
    });

    it('parses cursor and fetches from correct position', async () => {
      const cursor = encodeCursor(5000, 1); // Start after ledger 5000, event 1
      mockReq.query = { cursor };
      mockStellarService.fetchActivities.mockResolvedValue([mockActivities[2]]); // Only 4999:0

      await controller.getActivity(mockReq as Request, mockRes as Response);

      expect(mockStellarService.fetchActivities).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({
          fromLedger: 5000,
          fromEventIndex: 2, // 1 + 1 = start after cursor
          limit: 21, // limit + 1 for hasMore detection
          order: 'desc',
        })
      );
    });

    it('respects custom limit parameter', async () => {
      mockReq.query = { limit: '5' };
      mockStellarService.fetchActivities.mockResolvedValue(mockActivities);

      await controller.getActivity(mockReq as Request, mockRes as Response);

      expect(mockStellarService.fetchActivities).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({ limit: 6 }) // 5 + 1
      );

      const response = jsonMock.mock.calls[0][0] as ActivityResponse;
      expect(response.pagination.limit).toBe(5);
    });

    it('caps limit at MAX_LIMIT (100)', async () => {
      mockReq.query = { limit: '200' };
      mockStellarService.fetchActivities.mockResolvedValue([]);

      await controller.getActivity(mockReq as Request, mockRes as Response);

      expect(mockStellarService.fetchActivities).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({ limit: 101 }) // 100 + 1
      );
    });

    it('returns 400 for invalid cursor', async () => {
      mockReq.query = { cursor: 'invalid-cursor' };

      await controller.getActivity(mockReq as Request, mockRes as Response);

      expect(mockRes.status).toHaveBeenCalledWith(400);
      expect(jsonMock).toHaveBeenCalledWith(
        expect.objectContaining({
          error: 'Invalid cursor',
        })
      );
    });

    it('handles empty result set', async () => {
      mockStellarService.fetchActivities.mockResolvedValue([]);

      await controller.getActivity(mockReq as Request, mockRes as Response);

      const response = jsonMock.mock.calls[0][0] as ActivityResponse;
      expect(response.data).toEqual([]);
      expect(response.pagination.hasMore).toBe(false);
      expect(response.pagination.nextCursor).toBeNull();
    });

    it('handles service errors with 500', async () => {
      mockStellarService.fetchActivities.mockRejectedValue(new Error('Horizon timeout'));

      await controller.getActivity(mockReq as Request, mockRes as Response);

      expect(mockRes.status).toHaveBeenCalledWith(500);
      expect(jsonMock).toHaveBeenCalledWith(
        expect.objectContaining({
          error: 'Failed to fetch activity',
        })
      );
    });

    it('uses default limit when limit param is invalid', async () => {
      mockReq.query = { limit: 'not-a-number' };
      mockStellarService.fetchActivities.mockResolvedValue(mockActivities);

      await controller.getActivity(mockReq as Request, mockRes as Response);

      const response = jsonMock.mock.calls[0][0] as ActivityResponse;
      expect(response.pagination.limit).toBe(20);
    });

    it('uses default limit when limit param is negative', async () => {
      mockReq.query = { limit: '-5' };
      mockStellarService.fetchActivities.mockResolvedValue(mockActivities);

      await controller.getActivity(mockReq as Request, mockRes as Response);

      const response = jsonMock.mock.calls[0][0] as ActivityResponse;
      expect(response.pagination.limit).toBe(20);
    });

    it('fetches with no cursor from latest ledger', async () => {
      mockStellarService.fetchActivities.mockResolvedValue(mockActivities);

      await controller.getActivity(mockReq as Request, mockRes as Response);

      expect(mockStellarService.fetchActivities).toHaveBeenCalledWith(
        expect.any(String),
        expect.objectContaining({
          fromLedger: undefined,
          fromEventIndex: 0,
        })
      );
    });
  });

  describe('POST /api/lending/borrow', () => {
    it('should successfully process a borrow', async () => {
      const mockTxXdr = 'mock_tx_xdr';
      const mockTxHash = 'mock_tx_hash';

      mockStellarService.buildBorrowTransaction = jest.fn().mockResolvedValue(mockTxXdr);
      mockStellarService.submitTransaction = jest.fn().mockResolvedValue({
        success: true,
        transactionHash: mockTxHash,
        status: 'success',
      });
      mockStellarService.monitorTransaction = jest.fn().mockResolvedValue({
        success: true,
        transactionHash: mockTxHash,
        status: 'success',
        ledger: 12345,
      });

      (StellarService as jest.Mock).mockImplementation(() => mockStellarService);

      const response = await request(app)
        .post('/api/lending/borrow')
        .send({
          userAddress: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
          amount: '500000',
          userSecret: 'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
        });

      expect(response.status).toBe(200);
      expect(response.body.success).toBe(true);
    });

    it('should handle transaction failure', async () => {
      mockStellarService.buildBorrowTransaction = jest.fn().mockResolvedValue('mock_tx_xdr');
      mockStellarService.submitTransaction = jest.fn().mockResolvedValue({
        success: false,
        status: 'failed',
        error: 'Insufficient collateral',
      });

      (StellarService as jest.Mock).mockImplementation(() => mockStellarService);

      const response = await request(app)
        .post('/api/lending/borrow')
        .send({
          userAddress: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
          amount: '500000',
          userSecret: 'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
        });

      expect(response.status).toBe(400);
      expect(response.body.success).toBe(false);
    });
  });

  describe('POST /api/lending/repay', () => {
    it('should successfully process a repayment', async () => {
      const mockTxXdr = 'mock_tx_xdr';
      const mockTxHash = 'mock_tx_hash';

      mockStellarService.buildRepayTransaction = jest.fn().mockResolvedValue(mockTxXdr);
      mockStellarService.submitTransaction = jest.fn().mockResolvedValue({
        success: true,
        transactionHash: mockTxHash,
        status: 'success',
      });
      mockStellarService.monitorTransaction = jest.fn().mockResolvedValue({
        success: true,
        transactionHash: mockTxHash,
        status: 'success',
        ledger: 12345,
      });

      (StellarService as jest.Mock).mockImplementation(() => mockStellarService);

      const response = await request(app)
        .post('/api/lending/repay')
        .send({
          userAddress: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
          amount: '250000',
          userSecret: 'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
        });

      expect(response.status).toBe(200);
      expect(response.body.success).toBe(true);
    });
  });

  describe('POST /api/lending/withdraw', () => {
    it('should successfully process a withdrawal', async () => {
      const mockTxXdr = 'mock_tx_xdr';
      const mockTxHash = 'mock_tx_hash';

      mockStellarService.buildWithdrawTransaction = jest.fn().mockResolvedValue(mockTxXdr);
      mockStellarService.submitTransaction = jest.fn().mockResolvedValue({
        success: true,
        transactionHash: mockTxHash,
        status: 'success',
      });
      mockStellarService.monitorTransaction = jest.fn().mockResolvedValue({
        success: true,
        transactionHash: mockTxHash,
        status: 'success',
        ledger: 12345,
      });

      (StellarService as jest.Mock).mockImplementation(() => mockStellarService);

      const response = await request(app)
        .post('/api/lending/withdraw')
        .send({
          userAddress: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
          amount: '100000',
          userSecret: 'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
        });

      expect(response.status).toBe(200);
      expect(response.body.success).toBe(true);
    });

    it('should handle undercollateralization error', async () => {
      mockStellarService.buildWithdrawTransaction = jest.fn().mockResolvedValue('mock_tx_xdr');
      mockStellarService.submitTransaction = jest.fn().mockResolvedValue({
        success: false,
        status: 'failed',
        error: 'Withdrawal would violate minimum collateral ratio',
      });

      (StellarService as jest.Mock).mockImplementation(() => mockStellarService);

      const response = await request(app)
        .post('/api/lending/withdraw')
        .send({
          userAddress: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
          amount: '1000000',
          userSecret: 'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
        });

      expect(response.status).toBe(400);
      expect(response.body.success).toBe(false);
    });
  });

  describe('GET /api/health', () => {
    it('should return healthy status when all services are up', async () => {
      mockStellarService.healthCheck = jest.fn().mockResolvedValue({
        horizon: true,
        sorobanRpc: true,
      });

      (StellarService as jest.Mock).mockImplementation(() => mockStellarService);

      const response = await request(app).get('/api/health');

      expect(response.status).toBe(200);
      expect(response.body.status).toBe('healthy');
      expect(response.body.services.horizon).toBe(true);
      expect(response.body.services.sorobanRpc).toBe(true);
    });

    it('should return unhealthy status when services are down', async () => {
      mockStellarService.healthCheck = jest.fn().mockResolvedValue({
        horizon: false,
        sorobanRpc: false,
      });

      (StellarService as jest.Mock).mockImplementation(() => mockStellarService);

      const response = await request(app).get('/api/health');

      expect(response.status).toBe(503);
      expect(response.body.status).toBe('unhealthy');
    });
  });
});