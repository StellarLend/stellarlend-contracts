import request from 'supertest';
import { Request, Response } from 'express';
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
import type { LendingController as LendingControllerType, ActivityResponse } from '../controllers/lending.controller';

/**
 * Lending controller + cursor regression suite.
 *
 * Covers cursor encode/decode invariants, `getActivity` pagination/authorization
 * behaviour (invalid cursor -> 400, upstream failure -> 500, cursor resume,
 * limit clamping), and route-level authorization/validation for the lending
 * endpoints.
 */

const mockStellarService: any = {
  fetchActivityByLedgerRange: jest.fn(),
  fetchUserActivityByLedgerRange: jest.fn(),
  buildDepositTransaction: jest.fn(),
  buildBorrowTransaction: jest.fn(),
  buildRepayTransaction: jest.fn(),
  buildWithdrawTransaction: jest.fn(),
  submitTransaction: jest.fn(),
  monitorTransaction: jest.fn(),
  healthCheck: jest.fn(),
};

jest.mock('../services/stellar.service', () => ({
  StellarService: jest.fn(() => mockStellarService),
}));

// Required after the mock factory is registered so the mocked constructor can
// return `mockStellarService`.
const { LendingController } = require('../controllers/lending.controller');
const app = require('../app').default;

const VALID_USER_ADDRESS = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ';
const VALID_ASSET_ADDRESS = 'GD5TFY4DYYF43CQN3UMZUPBBXBLWK3WYAM5PIOMKOVRHBTZF7J7VGHP4';
const VALID_USER_SECRET = 'SAOS4OGIK6HD4QGR3DVRRDSR4FUBH73FCZGRZ7M53LRN67UQE5JDNS4I';

const makeActivity = (overrides: Record<string, unknown> = {}) => ({
  id: 'evt-1',
  type: 'borrow',
  user: VALID_USER_ADDRESS,
  amount: '100.0000000',
  asset: 'USDC',
  ledgerSequence: 5000,
  eventIndex: 2,
  timestamp: '2026-06-01T00:00:00.000Z',
  txHash: 'tx-abc',
  ...overrides,
});

const makeRes = () => {
  const res: any = {};
  res.status = jest.fn().mockReturnValue(res);
  res.json = jest.fn().mockReturnValue(res);
  return res;
};

beforeEach(() => {
  jest.clearAllMocks();
  mockStellarService.fetchActivityByLedgerRange.mockResolvedValue({ events: [], hasMore: false });
  mockStellarService.buildDepositTransaction.mockResolvedValue('xdr');
  mockStellarService.buildBorrowTransaction.mockResolvedValue('xdr');
  mockStellarService.buildRepayTransaction.mockResolvedValue('xdr');
  mockStellarService.buildWithdrawTransaction.mockResolvedValue('xdr');
  mockStellarService.submitTransaction.mockResolvedValue({
    success: false,
    status: 'failed',
    error: 'mock transaction failure',
  });
  mockStellarService.healthCheck.mockResolvedValue({ horizon: true, sorobanRpc: true });
});

describe('Cursor utilities', () => {
  it('round-trips a cursor', () => {
    const cursor = { ledgerSequence: 1000, eventIndex: 5 };
    expect(decodeCursor(encodeCursor(cursor))).toEqual(cursor);
  });

  it('encodes to url-safe base64 without padding', () => {
    const encoded = encodeCursor({ ledgerSequence: 1, eventIndex: 0 });
    expect(encoded).not.toContain('=');
    expect(encoded).not.toContain('+');
    expect(encoded).not.toContain('/');
  });

  it('rejects a negative ledger sequence', () => {
    expect(() => encodeCursor({ ledgerSequence: -1, eventIndex: 0 })).toThrow(CursorError);
  });

  it('rejects a negative event index', () => {
    expect(() => encodeCursor({ ledgerSequence: 0, eventIndex: -1 })).toThrow(CursorError);
  });

  it('rejects an empty cursor', () => {
    expect(() => decodeCursor('')).toThrow(CursorError);
  });

  it('rejects a cursor whose payload is not integer pairs', () => {
    const malformed = Buffer.from('abc:def', 'utf-8').toString('base64');
    expect(() => decodeCursor(malformed)).toThrow(CursorError);
  });

  it('isValidCursor rejects non-string / malformed values and accepts a real cursor', () => {
    expect(isValidCursor('not a cursor')).toBe(false);
    expect(isValidCursor(null)).toBe(false);
    expect(isValidCursor(123)).toBe(false);
    expect(isValidCursor(encodeCursor(1000, 0))).toBe(true);
  });

  it('nextCursor advances the event index', () => {
    expect(decodeCursor(nextCursor(1000, 5))).toEqual({ ledgerSequence: 1000, eventIndex: 6 });
  });

  it('sanitizePageSize clamps to [1, MAX_PAGE_SIZE] and defaults on invalid input', () => {
    expect(sanitizePageSize(undefined)).toBe(DEFAULT_PAGE_SIZE);
    expect(sanitizePageSize('0')).toBe(DEFAULT_PAGE_SIZE);
    expect(sanitizePageSize('-5')).toBe(DEFAULT_PAGE_SIZE);
    expect(sanitizePageSize('abc')).toBe(DEFAULT_PAGE_SIZE);
    expect(sanitizePageSize('5')).toBe(5);
    expect(sanitizePageSize('1000')).toBe(MAX_PAGE_SIZE);
  });
});

describe('LendingController.getActivity', () => {
  let controller: LendingControllerType;

  beforeEach(() => {
    controller = new LendingController(mockStellarService);
  });

  it('returns the first page with defaults when no query is supplied', async () => {
    mockStellarService.fetchActivityByLedgerRange.mockResolvedValue({
      events: [makeActivity()],
      hasMore: false,
    });
    const res = makeRes();

    await controller.getActivity({ query: {} } as unknown as Request, res as unknown as Response);

    expect(mockStellarService.fetchActivityByLedgerRange).toHaveBeenCalledWith({
      startLedger: null,
      startEventIndex: null,
      limit: 20,
    });
    const body = res.json.mock.calls[0][0] as ActivityResponse;
    expect(body.pagination).toEqual({ nextCursor: null, hasMore: false, limit: 20 });
  });

  it('maps service events into the response shape (user -> account)', async () => {
    mockStellarService.fetchActivityByLedgerRange.mockResolvedValue({
      events: [makeActivity()],
      hasMore: false,
    });
    const res = makeRes();

    await controller.getActivity({ query: {} } as unknown as Request, res as unknown as Response);

    const body = res.json.mock.calls[0][0] as ActivityResponse;
    expect(body.data[0]).toEqual({
      id: 'evt-1',
      type: 'borrow',
      ledgerSequence: 5000,
      eventIndex: 2,
      timestamp: '2026-06-01T00:00:00.000Z',
      amount: '100.0000000',
      asset: 'USDC',
      account: VALID_USER_ADDRESS,
      txHash: 'tx-abc',
    });
  });

  it('resumes strictly after the supplied cursor', async () => {
    const cursor = encodeCursor({ ledgerSequence: 4999, eventIndex: 3 });

    await controller.getActivity({ query: { cursor } } as unknown as Request, makeRes() as unknown as Response);

    expect(mockStellarService.fetchActivityByLedgerRange).toHaveBeenCalledWith({
      startLedger: 4999,
      startEventIndex: 4,
      limit: 20,
    });
  });

  it('reports hasMore and a decodable nextCursor', async () => {
    mockStellarService.fetchActivityByLedgerRange.mockResolvedValue({
      events: [makeActivity({ ledgerSequence: 5000, eventIndex: 7 })],
      hasMore: true,
    });
    const res = makeRes();

    await controller.getActivity({ query: {} } as unknown as Request, res as unknown as Response);

    const body = res.json.mock.calls[0][0] as ActivityResponse;
    expect(body.pagination.hasMore).toBe(true);
    expect(decodeCursor(body.pagination.nextCursor as string)).toEqual({
      ledgerSequence: 5000,
      eventIndex: 7,
    });
  });

  it('clamps an oversized limit to MAX_LIMIT and falls back to the default for non-positive limits', async () => {
    await controller.getActivity({ query: { limit: '9999' } } as unknown as Request, makeRes() as unknown as Response);
    expect(mockStellarService.fetchActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ limit: 100 })
    );

    mockStellarService.fetchActivityByLedgerRange.mockClear();
    await controller.getActivity({ query: { limit: '-1' } } as unknown as Request, makeRes() as unknown as Response);
    expect(mockStellarService.fetchActivityByLedgerRange).toHaveBeenCalledWith(
      expect.objectContaining({ limit: 20 })
    );
  });

  it('rejects an invalid cursor with 400 and never calls the service', async () => {
    const res = makeRes();

    await controller.getActivity({ query: { cursor: 'not-a-cursor' } } as unknown as Request, res as unknown as Response);

    expect(res.status).toHaveBeenCalledWith(400);
    expect(res.json).toHaveBeenCalledWith(
      expect.objectContaining({ error: 'Invalid cursor' })
    );
    expect(mockStellarService.fetchActivityByLedgerRange).not.toHaveBeenCalled();
  });

  it('returns 500 when the service fails', async () => {
    mockStellarService.fetchActivityByLedgerRange.mockRejectedValue(new Error('Horizon timeout'));
    const res = makeRes();

    await controller.getActivity({ query: {} } as unknown as Request, res as unknown as Response);

    expect(res.status).toHaveBeenCalledWith(500);
    expect(res.json).toHaveBeenCalledWith(
      expect.objectContaining({ error: 'Failed to fetch activity' })
    );
  });
});

describe('Route-level authorization and validation', () => {
  it('GET /api/lending/activity returns 200 with pagination metadata', async () => {
    mockStellarService.fetchActivityByLedgerRange.mockResolvedValue({
      events: [makeActivity()],
      hasMore: false,
    });

    const res = await request(app).get('/api/lending/activity');

    expect(res.status).toBe(200);
    expect(res.body.pagination.limit).toBe(20);
    expect(mockStellarService.fetchActivityByLedgerRange).toHaveBeenCalledWith({
      startLedger: null,
      startEventIndex: null,
      limit: 20,
    });
  });

  it('GET /api/lending/activity returns 400 for an invalid cursor', async () => {
    const res = await request(app).get('/api/lending/activity').query({ cursor: 'nope' });
    expect(res.status).toBe(400);
  });

  it('POST /api/lending/deposit rejects a missing userSecret (authorization) before the controller', async () => {
    const res = await request(app)
      .post('/api/lending/deposit')
      .send({ userAddress: VALID_USER_ADDRESS, amount: '1000000' });

    expect(res.status).toBe(400);
    expect(res.body.error).toContain('userSecret');
    expect(mockStellarService.buildDepositTransaction).not.toHaveBeenCalled();
  });

  it('POST /api/lending/deposit rejects a malformed userAddress before the controller', async () => {
    const res = await request(app)
      .post('/api/lending/deposit')
      .send({ userAddress: 'not-an-address', amount: '1000000', userSecret: VALID_USER_SECRET });

    expect(res.status).toBe(400);
    expect(mockStellarService.buildDepositTransaction).not.toHaveBeenCalled();
  });

  it('POST /api/lending/borrow validates and forwards a well-formed request', async () => {
    const res = await request(app)
      .post('/api/lending/borrow')
      .send({
        userAddress: VALID_USER_ADDRESS,
        assetAddress: VALID_ASSET_ADDRESS,
        amount: '500',
        userSecret: VALID_USER_SECRET,
      });

    expect(res.status).toBe(400);
    expect(res.body.error).toBe('mock transaction failure');
    expect(mockStellarService.buildBorrowTransaction).toHaveBeenCalledWith(
      VALID_USER_ADDRESS,
      VALID_ASSET_ADDRESS,
      '500',
      VALID_USER_SECRET
    );
  });

  it('POST /api/lending/repay rejects a non-positive amount', async () => {
    const res = await request(app)
      .post('/api/lending/repay')
      .send({ userAddress: VALID_USER_ADDRESS, amount: '0', userSecret: VALID_USER_SECRET });

    expect(res.status).toBe(400);
    expect(mockStellarService.buildRepayTransaction).not.toHaveBeenCalled();
  });

  it('POST /api/lending/withdraw rejects a missing userSecret', async () => {
    const res = await request(app)
      .post('/api/lending/withdraw')
      .send({ userAddress: VALID_USER_ADDRESS, amount: '100' });

    expect(res.status).toBe(400);
    expect(mockStellarService.buildWithdrawTransaction).not.toHaveBeenCalled();
  });

  it('GET /api/health returns 503 when a dependency is down', async () => {
    mockStellarService.healthCheck.mockResolvedValue({ horizon: false, sorobanRpc: true });

    const res = await request(app).get('/api/health');

    expect(res.status).toBe(503);
    expect(res.body.status).toBe('unhealthy');
  });
});
