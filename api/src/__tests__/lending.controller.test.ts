import { Request, Response } from 'express';
import { LendingController } from '../controllers/lending.controller';
import {
  DEFAULT_PAGE_SIZE,
  MAX_PAGE_SIZE,
  CursorError,
  decodeCursor,
  encodeCursor,
  isValidCursor,
  nextCursor,
} from '../utils/cursor';

describe('Cursor utilities', () => {
  it('round-trips a cursor', () => {
    const cursor = { ledgerSequence: 1000, eventIndex: 5 };
    expect(decodeCursor(encodeCursor(cursor))).toEqual(cursor);
  });

  it('rejects invalid cursor values', () => {
    expect(() => encodeCursor({ ledgerSequence: -1, eventIndex: 0 })).toThrow(CursorError);
    expect(() => encodeCursor({ ledgerSequence: 0, eventIndex: -1 })).toThrow(CursorError);
  });

  it('reports valid and invalid cursors', () => {
    expect(isValidCursor(encodeCursor({ ledgerSequence: 10, eventIndex: 3 }))).toBe(true);
    expect(isValidCursor('bad')).toBe(false);
  });

  it('uses the expected pagination defaults', () => {
    expect(DEFAULT_PAGE_SIZE).toBe(20);
    expect(MAX_PAGE_SIZE).toBe(100);
    expect(nextCursor(1000, 5)).toBeTruthy();
  });
});

describe('LendingController', () => {
  const createMockEvent = (overrides: Partial<Record<string, unknown>> = {}) => ({
    id: 'evt-1',
    type: 'borrow',
    user: 'GABC123',
    amount: '1000000000',
    asset: 'USDC',
    ledgerSequence: 1000,
    eventIndex: 5,
    timestamp: '2026-06-01T00:00:00Z',
    txHash: 'tx-abc',
    ...overrides,
  });

  const createMockRequest = (query: Record<string, unknown> = {}, params: Record<string, string> = {}) =>
    ({ query, params }) as Partial<Request>;

  const createMockResponse = () => {
    const res: any = {};
    res.status = jest.fn().mockReturnValue(res);
    res.json = jest.fn().mockReturnValue(res);
    return res as Partial<Response> & { json: jest.Mock; status: jest.Mock };
  };

  let controller: LendingController;
  let mockStellarService: {
    fetchActivityByLedgerRange: jest.Mock;
    fetchUserActivityByLedgerRange: jest.Mock;
  };

  beforeEach(() => {
    mockStellarService = {
      fetchActivityByLedgerRange: jest.fn(),
      fetchUserActivityByLedgerRange: jest.fn(),
    };
    controller = new LendingController(mockStellarService as any);
  });

  it('returns the first page of activity without a cursor', async () => {
    const events = [
      createMockEvent({ ledgerSequence: 1000, eventIndex: 0 }),
      createMockEvent({ ledgerSequence: 1000, eventIndex: 1 }),
    ];
    mockStellarService.fetchActivityByLedgerRange.mockResolvedValue({ events, hasMore: true });

    const req = createMockRequest({ limit: '2' });
    const res = createMockResponse();

    await controller.getActivity(req as Request, res as Response);

    expect(res.status).toHaveBeenCalledWith(200);
    const payload = (res.json as jest.Mock).mock.calls[0][0];
    expect(payload.data).toHaveLength(2);
    expect(payload.pagination.hasNextPage).toBe(true);
    expect(payload.pagination.nextCursor).not.toBeNull();
  });

  it('rejects invalid cursors', async () => {
    const req = createMockRequest({ cursor: 'bad' });
    const res = createMockResponse();

    await controller.getActivity(req as Request, res as Response);

    expect(res.status).toHaveBeenCalledWith(400);
    expect((res.json as jest.Mock).mock.calls[0][0]).toEqual(
      expect.objectContaining({ error: 'Invalid cursor' })
    );
  });

  it('uses the default page size when no limit is supplied', async () => {
    mockStellarService.fetchActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });

    const req = createMockRequest({});
    const res = createMockResponse();

    await controller.getActivity(req as Request, res as Response);

    expect(mockStellarService.fetchActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ limit: DEFAULT_PAGE_SIZE + 1 })
    );
  });

  it('caps page size', async () => {
    mockStellarService.fetchActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });

    const req = createMockRequest({ limit: '500' });
    const res = createMockResponse();

    await controller.getActivity(req as Request, res as Response);

    expect(mockStellarService.fetchActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ limit: MAX_PAGE_SIZE + 1 })
    );
  });

  it('returns the user-specific activity view', async () => {
    const userAddress = 'GABC123';
    const events = [createMockEvent({ user: userAddress, ledgerSequence: 120, eventIndex: 2 })];
    mockStellarService.fetchUserActivityByLedgerRange.mockResolvedValue({ events, hasMore: false });

    const req = createMockRequest({ limit: '10' }, { userAddress });
    const res = createMockResponse();

    await controller.getUserActivity(req as Request, res as Response);

    expect(res.status).toHaveBeenCalledWith(200);
    expect(mockStellarService.fetchUserActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ userAddress, limit: 11 })
    );
  });

  it('advances the cursor beyond the last event in a page', () => {
    expect(decodeCursor(nextCursor(1000, 5))).toEqual({ ledgerSequence: 1000, eventIndex: 6 });
  });
});
