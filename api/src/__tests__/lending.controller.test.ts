import request from 'supertest';
import { Request, Response } from 'express';
import app from '../app';
import { LendingController } from '../controllers/lending.controller';
import { StellarService } from '../services/stellar.service';
import {
  encodeCursor,
  decodeCursor,
  isValidCursor,
  sanitizePageSize,
  nextCursor,
  CursorError,
  DEFAULT_PAGE_SIZE,
  MAX_PAGE_SIZE,
} from '../utils/cursor';

// ---------------------------------------------------------------------------
// Mocks
// ---------------------------------------------------------------------------

// jest.mock is hoisted above all imports. The factory must be self-contained.
// Each new StellarService() returns a fresh object of jest.fns.
jest.mock('../services/stellar.service', () => ({
  StellarService: jest.fn().mockImplementation(() => ({
    fetchActivityByLedgerRange: jest.fn(),
    fetchUserActivityByLedgerRange: jest.fn(),
    buildDepositTransaction: jest.fn(),
    buildBorrowTransaction: jest.fn(),
    buildRepayTransaction: jest.fn(),
    buildWithdrawTransaction: jest.fn(),
    submitTransaction: jest.fn(),
    monitorTransaction: jest.fn(),
    healthCheck: jest.fn(),
    pingContract: jest.fn(),
  })),
}));

// The controller module creates a module-level StellarService singleton at
// import time. Supertest tests drive through that singleton.
// Unit tests inject their own fresh instance.
type MockSvc = {
  fetchActivityByLedgerRange: jest.Mock;
  fetchUserActivityByLedgerRange: jest.Mock;
  buildDepositTransaction: jest.Mock;
  buildBorrowTransaction: jest.Mock;
  buildRepayTransaction: jest.Mock;
  buildWithdrawTransaction: jest.Mock;
  submitTransaction: jest.Mock;
  monitorTransaction: jest.Mock;
  healthCheck: jest.Mock;
  pingContract: jest.Mock;
};

// The lending.controller module creates a StellarService singleton at load time.
// We can't rely on mock.instances because clearAllMocks() wipes that array.
// Instead we use a beforeAll in the integration test suites to capture it.
let _appSvc: MockSvc;

function appSvc(): MockSvc {
  return _appSvc;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function makeReq(
  query: Record<string, unknown> = {},
  params: Record<string, string> = {}
): Partial<Request> {
  return { query, params } as Partial<Request>;
}

function makeRes(): { res: Partial<Response>; json: jest.Mock; status: jest.Mock } {
  const res: any = {};
  res.json = jest.fn().mockReturnValue(res);
  res.status = jest.fn().mockReturnValue(res);
  return { res, json: res.json, status: res.status };
}

function mockEvent(overrides: Record<string, unknown> = {}) {
  return {
    id: 'evt-1',
    type: 'borrow',
    user: 'GABC123',
    amount: '1000000000',
    asset: 'USDC',
    ledgerSequence: 1000,
    eventIndex: 0,
    timestamp: '2026-01-01T00:00:00Z',
    txHash: 'tx-abc',
    ...overrides,
  };
}

// Real Stellar keypair for integration tests (passes the StellarAddress validator)
const VALID_ADDRESS = 'GA4Q2H7SGM7NQGJU6666RQKNVQFZRFK5M3ETRCAQRENNHHVFUXFFV32B';
const VALID_SECRET = 'SAHJ4ZDN7I2ZYW2HHXF2TA5JMILOHQB3X3KGE3BX6OFFV6HHJFGRTNRA';

// ---------------------------------------------------------------------------
// Cursor utilities
// ---------------------------------------------------------------------------

describe('cursor utilities', () => {
  describe('encodeCursor / decodeCursor', () => {
    it('round-trips a valid cursor', () => {
      const decoded = decodeCursor(encodeCursor({ ledgerSequence: 1000, eventIndex: 5 }));
      expect(decoded).toEqual({ ledgerSequence: 1000, eventIndex: 5 });
    });

    it('produces base64url output (no padding or standard base64 chars)', () => {
      const encoded = encodeCursor({ ledgerSequence: 1, eventIndex: 0 });
      expect(encoded).not.toMatch(/[=+/]/);
    });

    it('handles boundary values (ledger=u32max, eventIndex=MAX_EVENT_INDEX)', () => {
      const cursor = { ledgerSequence: 4_294_967_295, eventIndex: 1_000_000 };
      expect(decodeCursor(encodeCursor(cursor))).toEqual(cursor);
    });

    it('handles zero values', () => {
      expect(decodeCursor(encodeCursor({ ledgerSequence: 0, eventIndex: 0 }))).toEqual({
        ledgerSequence: 0,
        eventIndex: 0,
      });
    });

    it('throws CursorError for negative ledger sequence', () => {
      expect(() => encodeCursor({ ledgerSequence: -1, eventIndex: 0 })).toThrow(CursorError);
    });

    it('throws CursorError for negative event index', () => {
      expect(() => encodeCursor({ ledgerSequence: 0, eventIndex: -1 })).toThrow(CursorError);
    });

    it('throws CursorError when ledger exceeds u32 max', () => {
      expect(() => encodeCursor({ ledgerSequence: 4_294_967_296, eventIndex: 0 })).toThrow(CursorError);
    });

    it('throws CursorError when event index exceeds max', () => {
      expect(() => encodeCursor({ ledgerSequence: 0, eventIndex: 1_000_001 })).toThrow(CursorError);
    });

    it('throws CursorError for empty string', () => {
      expect(() => decodeCursor('')).toThrow(CursorError);
    });

    it('throws CursorError for null', () => {
      expect(() => decodeCursor(null as any)).toThrow(CursorError);
    });

    it('throws CursorError for undefined', () => {
      expect(() => decodeCursor(undefined as any)).toThrow(CursorError);
    });

    it('throws CursorError for malformed base64', () => {
      expect(() => decodeCursor('!!!not-base64!!!')).toThrow(CursorError);
    });

    it('throws CursorError when decoded value has no separator', () => {
      const noSep = Buffer.from('1000', 'utf-8').toString('base64url');
      expect(() => decodeCursor(noSep)).toThrow(CursorError);
    });

    it('throws CursorError for non-numeric parts', () => {
      const bad = Buffer.from('abc:def', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow(CursorError);
    });

    it('throws CursorError for out-of-range values in encoded string', () => {
      const outOfRange = Buffer.from('-1:0', 'utf-8').toString('base64url');
      expect(() => decodeCursor(outOfRange)).toThrow(CursorError);
    });
  });

  describe('isValidCursor', () => {
    it('returns true for a valid encoded cursor', () => {
      expect(isValidCursor(encodeCursor({ ledgerSequence: 100, eventIndex: 0 }))).toBe(true);
    });

    it('returns false for a garbage string', () => {
      expect(isValidCursor('not-a-cursor')).toBe(false);
    });

    it('returns false for a number', () => {
      expect(isValidCursor(123 as any)).toBe(false);
    });

    it('returns false for null', () => {
      expect(isValidCursor(null as any)).toBe(false);
    });

    it('returns false for undefined', () => {
      expect(isValidCursor(undefined as any)).toBe(false);
    });

    it('returns false for empty string', () => {
      expect(isValidCursor('')).toBe(false);
    });
  });

  describe('sanitizePageSize', () => {
    it('returns DEFAULT_PAGE_SIZE for undefined', () => {
      expect(sanitizePageSize(undefined)).toBe(DEFAULT_PAGE_SIZE);
    });

    it('returns DEFAULT_PAGE_SIZE for null', () => {
      expect(sanitizePageSize(null)).toBe(DEFAULT_PAGE_SIZE);
    });

    it('returns DEFAULT_PAGE_SIZE for NaN string', () => {
      expect(sanitizePageSize('abc')).toBe(DEFAULT_PAGE_SIZE);
    });

    it('returns DEFAULT_PAGE_SIZE for zero', () => {
      expect(sanitizePageSize(0)).toBe(DEFAULT_PAGE_SIZE);
    });

    it('returns DEFAULT_PAGE_SIZE for negative', () => {
      expect(sanitizePageSize(-10)).toBe(DEFAULT_PAGE_SIZE);
    });

    it('parses a valid string', () => {
      expect(sanitizePageSize('50')).toBe(50);
    });

    it('accepts a numeric value directly', () => {
      expect(sanitizePageSize(10)).toBe(10);
    });

    it('caps at MAX_PAGE_SIZE', () => {
      expect(sanitizePageSize(999)).toBe(MAX_PAGE_SIZE);
      expect(sanitizePageSize('999')).toBe(MAX_PAGE_SIZE);
    });

    it('accepts exactly MAX_PAGE_SIZE', () => {
      expect(sanitizePageSize(MAX_PAGE_SIZE)).toBe(MAX_PAGE_SIZE);
    });

    it('accepts 1 (minimum valid value)', () => {
      expect(sanitizePageSize(1)).toBe(1);
    });
  });

  describe('nextCursor', () => {
    it('increments event index by one', () => {
      expect(decodeCursor(nextCursor(1000, 5))).toEqual({ ledgerSequence: 1000, eventIndex: 6 });
    });

    it('works at event index 0', () => {
      expect(decodeCursor(nextCursor(500, 0))).toEqual({ ledgerSequence: 500, eventIndex: 1 });
    });
  });

  describe('ordering invariant', () => {
    it('cursor keys remain monotonically increasing across ledger boundaries', () => {
      const positions = [
        { ledgerSequence: 100, eventIndex: 5 },
        { ledgerSequence: 100, eventIndex: 10 },
        { ledgerSequence: 101, eventIndex: 0 },
        { ledgerSequence: 102, eventIndex: 3 },
      ];
      const keys = positions.map((p) => p.ledgerSequence * 1_000_001 + p.eventIndex);
      for (let i = 0; i < keys.length - 1; i++) {
        expect(keys[i]).toBeLessThan(keys[i + 1]);
      }
    });
  });
});

// ---------------------------------------------------------------------------
// LendingController.getActivity — unit tests (injected mock)
// ---------------------------------------------------------------------------

describe('LendingController.getActivity', () => {
  let controller: LendingController;
  let svc: MockSvc;

  beforeEach(() => {
    jest.clearAllMocks();
    // Create a fresh mock service instance and capture its return value
    new StellarService('', '');
    svc = (StellarService as jest.Mock).mock.results[0].value as unknown as MockSvc;
    controller = new LendingController(svc as unknown as StellarService);
  });

  it('returns first page without a cursor', async () => {
    const events = [
      mockEvent({ ledgerSequence: 1000, eventIndex: 0 }),
      mockEvent({ id: 'evt-2', ledgerSequence: 1000, eventIndex: 1 }),
    ];
    svc.fetchActivityByLedgerRange.mockResolvedValue({ events, hasMore: false });

    const { res, status, json } = makeRes();
    await controller.getActivity(makeReq() as Request, res as Response);

    expect(status).toHaveBeenCalledWith(200);
    const body = json.mock.calls[0][0];
    expect(body.data).toHaveLength(2);
    expect(body.pagination.hasNextPage).toBe(false);
    expect(body.pagination.nextCursor).toBeNull();
  });

  it('passes default page size + 1 to the service', async () => {
    svc.fetchActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });
    const { res } = makeRes();
    await controller.getActivity(makeReq() as Request, res as Response);
    expect(svc.fetchActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ limit: DEFAULT_PAGE_SIZE + 1 })
    );
  });

  it('caps limit at MAX_PAGE_SIZE and passes MAX + 1 to service', async () => {
    svc.fetchActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });
    const { res } = makeRes();
    await controller.getActivity(makeReq({ limit: '999' }) as Request, res as Response);
    expect(svc.fetchActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ limit: MAX_PAGE_SIZE + 1 })
    );
  });

  it('passes null cursor fields when no cursor provided', async () => {
    svc.fetchActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });
    const { res } = makeRes();
    await controller.getActivity(makeReq() as Request, res as Response);
    expect(svc.fetchActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ startLedger: null, startEventIndex: null })
    );
  });

  it('decodes cursor and passes ledger/eventIndex to service', async () => {
    const cursor = encodeCursor({ ledgerSequence: 500, eventIndex: 3 });
    svc.fetchActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });
    const { res } = makeRes();
    await controller.getActivity(makeReq({ cursor }) as Request, res as Response);
    expect(svc.fetchActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ startLedger: 500, startEventIndex: 3 })
    );
  });

  it('generates a correct nextCursor when hasNextPage is true', async () => {
    const events = Array.from({ length: 3 }, (_, i) =>
      mockEvent({ id: `evt-${i}`, ledgerSequence: 100, eventIndex: i })
    );
    svc.fetchActivityByLedgerRange.mockResolvedValue({ events, hasMore: true });

    const { res, json } = makeRes();
    await controller.getActivity(makeReq({ limit: '2' }) as Request, res as Response);

    const body = json.mock.calls[0][0];
    expect(body.pagination.hasNextPage).toBe(true);
    expect(body.data).toHaveLength(2);
    // nextCursor encodes eventIndex + 1 of the last returned item
    expect(decodeCursor(body.pagination.nextCursor)).toEqual({ ledgerSequence: 100, eventIndex: 2 });
  });

  it('returns empty data with no nextCursor for empty result', async () => {
    svc.fetchActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });
    const { res, status, json } = makeRes();
    await controller.getActivity(makeReq() as Request, res as Response);
    expect(status).toHaveBeenCalledWith(200);
    const body = json.mock.calls[0][0];
    expect(body.data).toEqual([]);
    expect(body.pagination.nextCursor).toBeNull();
    expect(body.pagination.hasNextPage).toBe(false);
  });

  it('returns 400 for an invalid cursor string', async () => {
    const { res, status, json } = makeRes();
    await controller.getActivity(makeReq({ cursor: 'not-valid' }) as Request, res as Response);
    expect(status).toHaveBeenCalledWith(400);
    expect(json).toHaveBeenCalledWith(
      expect.objectContaining({ error: 'Invalid cursor', code: 'INVALID_CURSOR' })
    );
    expect(svc.fetchActivityByLedgerRange).not.toHaveBeenCalled();
  });

  it('returns 400 for a structurally invalid base64url cursor', async () => {
    const { res, status } = makeRes();
    await controller.getActivity(makeReq({ cursor: '!!!###' }) as Request, res as Response);
    expect(status).toHaveBeenCalledWith(400);
    expect(svc.fetchActivityByLedgerRange).not.toHaveBeenCalled();
  });

  it('returns 400 when service throws CursorError', async () => {
    const cursor = encodeCursor({ ledgerSequence: 100, eventIndex: 0 });
    svc.fetchActivityByLedgerRange.mockRejectedValue(new CursorError('cursor expired'));
    const { res, status, json } = makeRes();
    await controller.getActivity(makeReq({ cursor }) as Request, res as Response);
    expect(status).toHaveBeenCalledWith(400);
    expect(json).toHaveBeenCalledWith(expect.objectContaining({ code: 'INVALID_CURSOR' }));
  });

  it('returns 500 when the service throws an unexpected error', async () => {
    svc.fetchActivityByLedgerRange.mockRejectedValue(new Error('RPC timeout'));
    const { res, status, json } = makeRes();
    await controller.getActivity(makeReq() as Request, res as Response);
    expect(status).toHaveBeenCalledWith(500);
    expect(json).toHaveBeenCalledWith(
      expect.objectContaining({ error: 'Internal server error', code: 'INTERNAL_ERROR' })
    );
  });

  it('returns 500 when the service rejects with a non-Error value', async () => {
    svc.fetchActivityByLedgerRange.mockRejectedValue('string error');
    const { res, status } = makeRes();
    await controller.getActivity(makeReq() as Request, res as Response);
    expect(status).toHaveBeenCalledWith(500);
  });

  it('does not leak internal error details in the 500 response', async () => {
    svc.fetchActivityByLedgerRange.mockRejectedValue(new Error('secret internal details'));
    const { res, json } = makeRes();
    await controller.getActivity(makeReq() as Request, res as Response);
    expect(JSON.stringify(json.mock.calls[0][0])).not.toContain('secret internal details');
  });

  it('produces no duplicate or missing events across two pages', async () => {
    const allEvents = Array.from({ length: 5 }, (_, i) =>
      mockEvent({ id: `evt-${i}`, ledgerSequence: 100 + Math.floor(i / 2), eventIndex: i % 2 })
    );

    // Page 1: controller requests limit+1=3, gets 3 → hasNextPage=true, returns 2
    svc.fetchActivityByLedgerRange.mockResolvedValueOnce({ events: allEvents.slice(0, 3), hasMore: true });
    const { res: res1, json: json1 } = makeRes();
    await controller.getActivity(makeReq({ limit: '2' }) as Request, res1 as Response);
    const page1 = json1.mock.calls[0][0];
    expect(page1.data).toHaveLength(2);
    expect(page1.pagination.hasNextPage).toBe(true);

    // Page 2: service returns 2 events for limit=2, no more after
    svc.fetchActivityByLedgerRange.mockResolvedValueOnce({ events: allEvents.slice(2, 4), hasMore: false });
    const { res: res2, json: json2 } = makeRes();
    await controller.getActivity(
      makeReq({ cursor: page1.pagination.nextCursor, limit: '2' }) as Request,
      res2 as Response
    );
    const page2 = json2.mock.calls[0][0];

    const page1Ids = page1.data.map((e: any) => e.id);
    const page2Ids = page2.data.map((e: any) => e.id);

    // No overlap between pages
    expect(page1Ids.filter((id: string) => page2Ids.includes(id))).toHaveLength(0);
    // Together they cover 4 unique event ids (2 per page)
    expect(new Set([...page1Ids, ...page2Ids]).size).toBe(4);
    // Cursor from page1 was passed to page2 service call
    expect(svc.fetchActivityByLedgerRange).toHaveBeenLastCalledWith(
      expect.objectContaining({ startLedger: 100, startEventIndex: 2 })
    );
  });

  it('ignores limit=0 and uses default page size', async () => {
    svc.fetchActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });
    const { res } = makeRes();
    await controller.getActivity(makeReq({ limit: '0' }) as Request, res as Response);
    expect(svc.fetchActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ limit: DEFAULT_PAGE_SIZE + 1 })
    );
  });

  it('ignores negative limit and uses default page size', async () => {
    svc.fetchActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });
    const { res } = makeRes();
    await controller.getActivity(makeReq({ limit: '-5' }) as Request, res as Response);
    expect(svc.fetchActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ limit: DEFAULT_PAGE_SIZE + 1 })
    );
  });

  it('ignores non-numeric limit and uses default page size', async () => {
    svc.fetchActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });
    const { res } = makeRes();
    await controller.getActivity(makeReq({ limit: 'banana' }) as Request, res as Response);
    expect(svc.fetchActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ limit: DEFAULT_PAGE_SIZE + 1 })
    );
  });
});

// ---------------------------------------------------------------------------
// LendingController.getUserActivity — unit tests (injected mock)
// ---------------------------------------------------------------------------

describe('LendingController.getUserActivity', () => {
  let controller: LendingController;
  let svc: MockSvc;

  beforeEach(() => {
    jest.clearAllMocks();
    new StellarService('', '');
    svc = (StellarService as jest.Mock).mock.results[0].value as unknown as MockSvc;
    controller = new LendingController(svc as unknown as StellarService);
  });

  it('returns 400 when userAddress param is missing', async () => {
    const { res, status, json } = makeRes();
    await controller.getUserActivity(makeReq({}, {}) as Request, res as Response);
    expect(status).toHaveBeenCalledWith(400);
    expect(json).toHaveBeenCalledWith(
      expect.objectContaining({ error: 'Invalid user address', code: 'INVALID_ADDRESS' })
    );
    expect(svc.fetchUserActivityByLedgerRange).not.toHaveBeenCalled();
  });

  it('fetches user-specific events and returns 200', async () => {
    const userAddress = 'GABC123';
    svc.fetchUserActivityByLedgerRange.mockResolvedValue({
      events: [mockEvent({ user: userAddress })],
      hasMore: false,
    });
    const { res, status, json } = makeRes();
    await controller.getUserActivity(makeReq({}, { userAddress }) as Request, res as Response);
    expect(status).toHaveBeenCalledWith(200);
    expect(json.mock.calls[0][0].data).toHaveLength(1);
  });

  it('passes userAddress and null cursor fields when no cursor provided', async () => {
    const userAddress = 'GABC123';
    svc.fetchUserActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });
    const { res } = makeRes();
    await controller.getUserActivity(makeReq({}, { userAddress }) as Request, res as Response);
    expect(svc.fetchUserActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ userAddress, startLedger: null, startEventIndex: null })
    );
  });

  it('decodes cursor and forwards ledger position for user activity', async () => {
    const userAddress = 'GABC123';
    const cursor = encodeCursor({ ledgerSequence: 200, eventIndex: 7 });
    svc.fetchUserActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });
    const { res } = makeRes();
    await controller.getUserActivity(makeReq({ cursor }, { userAddress }) as Request, res as Response);
    expect(svc.fetchUserActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ startLedger: 200, startEventIndex: 7 })
    );
  });

  it('returns 400 for invalid cursor on user activity endpoint', async () => {
    const { res, status } = makeRes();
    await controller.getUserActivity(
      makeReq({ cursor: 'garbage' }, { userAddress: 'GABC123' }) as Request,
      res as Response
    );
    expect(status).toHaveBeenCalledWith(400);
    expect(svc.fetchUserActivityByLedgerRange).not.toHaveBeenCalled();
  });

  it('returns 500 when user activity service throws', async () => {
    svc.fetchUserActivityByLedgerRange.mockRejectedValue(new Error('network error'));
    const { res, status } = makeRes();
    await controller.getUserActivity(
      makeReq({}, { userAddress: 'GABC123' }) as Request,
      res as Response
    );
    expect(status).toHaveBeenCalledWith(500);
  });

  it('generates nextCursor for user activity when more results exist', async () => {
    const userAddress = 'GABC123';
    const events = [
      mockEvent({ id: 'u1', user: userAddress, ledgerSequence: 300, eventIndex: 0 }),
      mockEvent({ id: 'u2', user: userAddress, ledgerSequence: 300, eventIndex: 1 }),
      mockEvent({ id: 'u3', user: userAddress, ledgerSequence: 300, eventIndex: 2 }),
    ];
    svc.fetchUserActivityByLedgerRange.mockResolvedValue({ events, hasMore: true });

    const { res, json } = makeRes();
    await controller.getUserActivity(makeReq({ limit: '2' }, { userAddress }) as Request, res as Response);

    const body = json.mock.calls[0][0];
    expect(body.pagination.hasNextPage).toBe(true);
    expect(body.data).toHaveLength(2);
    expect(decodeCursor(body.pagination.nextCursor)).toEqual({ ledgerSequence: 300, eventIndex: 2 });
  });
});

// ---------------------------------------------------------------------------
// Standalone route handlers — supertest integration tests
// ---------------------------------------------------------------------------

// Capture the module-level StellarService singleton once, before any
// clearAllMocks() call wipes mock.instances. The first instance created is
// the one inside lending.controller.ts (created at module load via `import app`).
beforeAll(() => {
  // mock.results[0].value gives the actual object returned by the factory
  // (mock.instances[0] is the 'this' object, which is different from the factory return value)
  _appSvc = (StellarService as jest.Mock).mock.results[0].value as unknown as MockSvc;
  if (!_appSvc) {
    throw new Error('StellarService mock result not found. The module-level singleton in lending.controller.ts may not be using the mocked StellarService.');
  }
});

describe('POST /api/lending/deposit', () => {
  beforeEach(() => jest.clearAllMocks());

  it('returns 200 on a successful deposit', async () => {
    appSvc().buildDepositTransaction.mockResolvedValue('tx_xdr');
    appSvc().submitTransaction.mockResolvedValue({ success: true, transactionHash: 'hash1', status: 'success' });
    appSvc().monitorTransaction.mockResolvedValue({ success: true, transactionHash: 'hash1', status: 'success', ledger: 1000 });

    const res = await request(app).post('/api/lending/deposit').send({
      userAddress: VALID_ADDRESS,
      amount: '1000000',
      userSecret: VALID_SECRET,
    });

    expect(res.status).toBe(200);
    expect(res.body.success).toBe(true);
  });

  it('returns 400 when the transaction fails on-chain', async () => {
    appSvc().buildDepositTransaction.mockResolvedValue('tx_xdr');
    appSvc().submitTransaction.mockResolvedValue({ success: false, status: 'failed', error: 'Insufficient funds' });

    const res = await request(app).post('/api/lending/deposit').send({
      userAddress: VALID_ADDRESS,
      amount: '1000000',
      userSecret: VALID_SECRET,
    });

    expect(res.status).toBe(400);
    expect(res.body.success).toBe(false);
  });

  it('returns 400 when amount is missing', async () => {
    const res = await request(app).post('/api/lending/deposit').send({
      userAddress: VALID_ADDRESS,
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(400);
  });

  it('returns 400 when amount is zero', async () => {
    const res = await request(app).post('/api/lending/deposit').send({
      userAddress: VALID_ADDRESS,
      amount: '0',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(400);
  });

  it('returns 400 when amount is negative', async () => {
    const res = await request(app).post('/api/lending/deposit').send({
      userAddress: VALID_ADDRESS,
      amount: '-1',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(400);
  });

  it('returns 400 when amount is not an integer string', async () => {
    const res = await request(app).post('/api/lending/deposit').send({
      userAddress: VALID_ADDRESS,
      amount: '1.5',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(400);
  });

  it('returns 400 when userAddress is invalid', async () => {
    const res = await request(app).post('/api/lending/deposit').send({
      userAddress: 'not-an-address',
      amount: '1000000',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(400);
  });

  it('returns 400 when userSecret is empty', async () => {
    const res = await request(app).post('/api/lending/deposit').send({
      userAddress: VALID_ADDRESS,
      amount: '1000000',
      userSecret: '',
    });
    expect(res.status).toBe(400);
  });

  it('returns 500 when buildDepositTransaction throws', async () => {
    appSvc().buildDepositTransaction.mockRejectedValue(new Error('Soroban RPC unavailable'));

    const res = await request(app).post('/api/lending/deposit').send({
      userAddress: VALID_ADDRESS,
      amount: '1000000',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(500);
  });

  it('returns 500 when submitTransaction throws', async () => {
    appSvc().buildDepositTransaction.mockResolvedValue('tx_xdr');
    appSvc().submitTransaction.mockRejectedValue(new Error('Horizon unreachable'));

    const res = await request(app).post('/api/lending/deposit').send({
      userAddress: VALID_ADDRESS,
      amount: '1000000',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(500);
  });

  it('accepts the i128 max value as amount', async () => {
    const i128Max = ((1n << 127n) - 1n).toString();
    appSvc().buildDepositTransaction.mockResolvedValue('tx_xdr');
    appSvc().submitTransaction.mockResolvedValue({ success: true, transactionHash: 'h', status: 'success' });
    appSvc().monitorTransaction.mockResolvedValue({ success: true, transactionHash: 'h', status: 'success', ledger: 1 });

    const res = await request(app).post('/api/lending/deposit').send({
      userAddress: VALID_ADDRESS,
      amount: i128Max,
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(200);
  });

  it('rejects an amount exceeding i128 max', async () => {
    const tooBig = (1n << 127n).toString();
    const res = await request(app).post('/api/lending/deposit').send({
      userAddress: VALID_ADDRESS,
      amount: tooBig,
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(400);
  });
});

describe('POST /api/lending/borrow', () => {
  beforeEach(() => jest.clearAllMocks());

  it('returns 200 on a successful borrow', async () => {
    appSvc().buildBorrowTransaction.mockResolvedValue('tx_xdr');
    appSvc().submitTransaction.mockResolvedValue({ success: true, transactionHash: 'h2', status: 'success' });
    appSvc().monitorTransaction.mockResolvedValue({ success: true, transactionHash: 'h2', status: 'success', ledger: 1001 });

    const res = await request(app).post('/api/lending/borrow').send({
      userAddress: VALID_ADDRESS,
      amount: '500000',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(200);
    expect(res.body.success).toBe(true);
  });

  it('returns 400 when on-chain borrow fails (insufficient collateral)', async () => {
    appSvc().buildBorrowTransaction.mockResolvedValue('tx_xdr');
    appSvc().submitTransaction.mockResolvedValue({ success: false, status: 'failed', error: 'Insufficient collateral' });

    const res = await request(app).post('/api/lending/borrow').send({
      userAddress: VALID_ADDRESS,
      amount: '500000',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(400);
  });

  it('returns 400 for missing required fields', async () => {
    const res = await request(app).post('/api/lending/borrow').send({ userAddress: VALID_ADDRESS });
    expect(res.status).toBe(400);
  });

  it('returns 500 when service throws during borrow', async () => {
    appSvc().buildBorrowTransaction.mockRejectedValue(new Error('contract call failed'));

    const res = await request(app).post('/api/lending/borrow').send({
      userAddress: VALID_ADDRESS,
      amount: '500000',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(500);
  });
});

describe('POST /api/lending/repay', () => {
  beforeEach(() => jest.clearAllMocks());

  it('returns 200 on a successful repayment', async () => {
    appSvc().buildRepayTransaction.mockResolvedValue('tx_xdr');
    appSvc().submitTransaction.mockResolvedValue({ success: true, transactionHash: 'h3', status: 'success' });
    appSvc().monitorTransaction.mockResolvedValue({ success: true, transactionHash: 'h3', status: 'success', ledger: 1002 });

    const res = await request(app).post('/api/lending/repay').send({
      userAddress: VALID_ADDRESS,
      amount: '250000',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(200);
    expect(res.body.success).toBe(true);
  });

  it('returns 400 for zero repayment amount', async () => {
    const res = await request(app).post('/api/lending/repay').send({
      userAddress: VALID_ADDRESS,
      amount: '0',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(400);
  });

  it('returns 400 when on-chain repay fails', async () => {
    appSvc().buildRepayTransaction.mockResolvedValue('tx_xdr');
    appSvc().submitTransaction.mockResolvedValue({ success: false, status: 'failed', error: 'Repay exceeds debt' });

    const res = await request(app).post('/api/lending/repay').send({
      userAddress: VALID_ADDRESS,
      amount: '999999999',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(400);
  });
});

describe('POST /api/lending/withdraw', () => {
  beforeEach(() => jest.clearAllMocks());

  it('returns 200 on a successful withdrawal', async () => {
    appSvc().buildWithdrawTransaction.mockResolvedValue('tx_xdr');
    appSvc().submitTransaction.mockResolvedValue({ success: true, transactionHash: 'h4', status: 'success' });
    appSvc().monitorTransaction.mockResolvedValue({ success: true, transactionHash: 'h4', status: 'success', ledger: 1003 });

    const res = await request(app).post('/api/lending/withdraw').send({
      userAddress: VALID_ADDRESS,
      amount: '100000',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(200);
  });

  it('returns 400 when withdrawal violates collateral ratio', async () => {
    appSvc().buildWithdrawTransaction.mockResolvedValue('tx_xdr');
    appSvc().submitTransaction.mockResolvedValue({
      success: false,
      status: 'failed',
      error: 'Withdrawal would violate minimum collateral ratio',
    });

    const res = await request(app).post('/api/lending/withdraw').send({
      userAddress: VALID_ADDRESS,
      amount: '99999999',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(400);
  });

  it('returns 500 when service throws during withdraw', async () => {
    appSvc().buildWithdrawTransaction.mockRejectedValue(new Error('unexpected'));

    const res = await request(app).post('/api/lending/withdraw').send({
      userAddress: VALID_ADDRESS,
      amount: '100000',
      userSecret: VALID_SECRET,
    });
    expect(res.status).toBe(500);
  });
});

// ---------------------------------------------------------------------------
// Health endpoints
// ---------------------------------------------------------------------------

describe('GET /api/health', () => {
  beforeEach(() => jest.clearAllMocks());

  it('returns 200 when all services are up', async () => {
    appSvc().healthCheck.mockResolvedValue({ horizon: true, sorobanRpc: true });

    const res = await request(app).get('/api/health');

    expect(res.status).toBe(200);
    expect(res.body.status).toBe('healthy');
    expect(res.body.services.horizon).toBe(true);
    expect(res.body.services.sorobanRpc).toBe(true);
  });

  it('returns 503 when a service is down', async () => {
    appSvc().healthCheck.mockResolvedValue({ horizon: true, sorobanRpc: false });

    const res = await request(app).get('/api/health');

    expect(res.status).toBe(503);
    expect(res.body.status).toBe('unhealthy');
  });

  it('returns 500 when health check throws', async () => {
    appSvc().healthCheck.mockRejectedValue(new Error('Horizon unreachable'));

    const res = await request(app).get('/api/health');

    expect(res.status).toBe(500);
  });
});
