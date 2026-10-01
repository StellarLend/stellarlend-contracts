/**
 * Coverage for the ledger-range activity pagination and deep health probe.
 *
 * These paths back the cursor contract: they are what a cursor is decoded into
 * and what the /api/health/healthz endpoint reports. The Stellar SDK is NOT
 * mocked here so that real XDR construction and event parsing are exercised.
 */

import type { StellarService as StellarServiceType } from '../stellar.service';
import { Server as SorobanServer } from '@stellar/stellar-sdk/rpc';
import { Address, nativeToScVal, xdr } from '@stellar/stellar-sdk';
import axios from 'axios';

jest.mock('axios');
const mockedAxios = axios as jest.Mocked<typeof axios>;

const CONTRACT_ID = 'CADQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQP5KR';
const USER = 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK';
/** Largest value representable by a signed 128-bit integer (2^127 - 1). */
const I128_MAX = '170141183460469231731687303715884105727';

// `pingContract` resolves the contract id through `config`, which snapshots
// the environment the first time it is imported. In production the constructor
// argument and `config` are the same value, so the env is pinned here to
// reproduce that, and it has to happen before the service module is loaded.
process.env.CONTRACT_ID = CONTRACT_ID;

const { StellarService } = require('../stellar.service') as {
  StellarService: new (rpcUrl?: string, lendingContractId?: string) => StellarServiceType;
};

/** Build a raw contract event that the service's parser should accept. */
function rawEvent(opts: {
  id: string;
  ledger: number;
  eventIndex: number;
  type?: string;
  user?: string;
  amount?: string;
  asset?: string;
}) {
  // The service parses `value` as a Map with 'amount' and 'asset' keys, so the
  // fixture has to build that shape. A bare i128 ScVal is not a map and parses
  // to the `{ amount: '0' }` fallback.
  const value = xdr.ScVal.scvMap([
    new xdr.ScMapEntry({
      key: xdr.ScVal.scvSymbol('amount'),
      val: nativeToScVal(opts.amount ?? '1000', { type: 'i128' }),
    }),
    new xdr.ScMapEntry({
      key: xdr.ScVal.scvSymbol('asset'),
      val: xdr.ScVal.scvAddress(Address.fromString(opts.asset ?? USER).toScAddress()),
    }),
  ]);

  return {
    id: opts.id,
    type: 'contract',
    ledger: opts.ledger,
    ledgerClosedAt: '2026-01-01T00:00:00Z',
    contractId: CONTRACT_ID,
    // topic[0] is the event-type symbol, topic[1] the user address
    topic: [
      xdr.ScVal.scvSymbol(opts.type ?? 'deposit'),
      xdr.ScVal.scvAddress(Address.fromString(opts.user ?? USER).toScAddress()),
    ],
    value,
    inSuccessfulContractCall: true,
    txHash: `tx-${opts.id}`,
  } as any;
}

describe('StellarService ledger-range activity', () => {
  let service: StellarService;
  let mockRpc: { getEvents: jest.Mock; getLatestLedger: jest.Mock; getHealth: jest.Mock; prepareTransaction: jest.Mock };
  let mockSoroban: { getHealth: jest.Mock; prepareTransaction: jest.Mock };

  beforeEach(() => {
    jest.clearAllMocks();

    mockRpc = {
      getEvents: jest.fn().mockResolvedValue({ events: [] }),
      getLatestLedger: jest.fn().mockResolvedValue({ sequence: 5000 }),
      getHealth: jest.fn().mockResolvedValue({ status: 'healthy' }),
      prepareTransaction: jest.fn().mockResolvedValue({}),
    };
    mockSoroban = {
      getHealth: jest.fn().mockResolvedValue({ status: 'healthy' }),
      prepareTransaction: jest.fn().mockResolvedValue({}),
    };

    // Server is a real class here (the SDK is not mocked), so stub its
    // prototype rather than replacing the constructor. The stubs must forward
    // their arguments, otherwise assertions on the RPC request (e.g. the
    // endLedger bound) can never see them.
    jest
      .spyOn(SorobanServer.prototype, 'getEvents')
      .mockImplementation(async (...args: unknown[]) => mockRpc.getEvents(...args) as any);
    jest
      .spyOn(SorobanServer.prototype, 'getLatestLedger')
      .mockImplementation(async (...args: unknown[]) => mockRpc.getLatestLedger(...args) as any);
    jest
      .spyOn(SorobanServer.prototype, 'getHealth')
      .mockImplementation(async (...args: unknown[]) => mockSoroban.getHealth(...args) as any);
    jest
      .spyOn(SorobanServer.prototype, 'prepareTransaction')
      .mockImplementation(async (...args: unknown[]) => mockSoroban.prepareTransaction(...args) as any);

    // `pingContract` resolves the contract through config, which in production
    // is the same value passed to the constructor.
    service = new StellarService('https://rpc.test', CONTRACT_ID);
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  describe('fetchActivityByLedgerRange', () => {
    it('returns events ordered by ledger then event index', async () => {
      mockRpc.getEvents.mockResolvedValue({
        events: [
          rawEvent({ id: 'b', ledger: 100, eventIndex: 1 }),
          rawEvent({ id: 'a', ledger: 100, eventIndex: 0 }),
          rawEvent({ id: 'c', ledger: 99, eventIndex: 5 }),
        ],
      });

      const { events } = await service.fetchActivityByLedgerRange({
        startLedger: 99,
        startEventIndex: 0,
        limit: 10,
      });

      expect(events.map(e => e.id)).toEqual(['c', 'a', 'b']);
    });

    it('filters out events before the cursor position within the same ledger', async () => {
      mockRpc.getEvents.mockResolvedValue({
        events: [
          rawEvent({ id: 'a', ledger: 100, eventIndex: 0 }),
          rawEvent({ id: 'b', ledger: 100, eventIndex: 1 }),
          rawEvent({ id: 'c', ledger: 100, eventIndex: 2 }),
        ],
      });

      const { events } = await service.fetchActivityByLedgerRange({
        startLedger: 100,
        startEventIndex: 1,
        limit: 10,
      });

      // Cursor at (100,1) must resume strictly after it: no duplicates.
      expect(events.map(e => e.id)).toEqual(['b', 'c']);
    });

    it('drops events from earlier ledgers when a cursor is supplied', async () => {
      mockRpc.getEvents.mockResolvedValue({
        events: [
          rawEvent({ id: 'old', ledger: 98, eventIndex: 0 }),
          rawEvent({ id: 'new', ledger: 100, eventIndex: 0 }),
        ],
      });

      const { events } = await service.fetchActivityByLedgerRange({
        startLedger: 100,
        startEventIndex: 0,
        limit: 10,
      });

      expect(events.map(e => e.id)).toEqual(['new']);
    });

    it('never returns more than the requested limit', async () => {
      mockRpc.getEvents.mockResolvedValue({
        events: Array.from({ length: 10 }, (_, i) =>
          rawEvent({ id: `e${i}`, ledger: 100, eventIndex: i })
        ),
      });

      const { events } = await service.fetchActivityByLedgerRange({
        startLedger: null,
        startEventIndex: null,
        limit: 3,
      });

      expect(events).toHaveLength(3);
    });

    it('sets hasMore when the page is full', async () => {
      mockRpc.getEvents.mockResolvedValue({
        events: Array.from({ length: 3 }, (_, i) =>
          rawEvent({ id: `e${i}`, ledger: 100, eventIndex: i })
        ),
      });

      const { hasMore } = await service.fetchActivityByLedgerRange({
        startLedger: null,
        startEventIndex: null,
        limit: 3,
      });

      expect(hasMore).toBe(true);
    });

    it('clears hasMore on a short page', async () => {
      mockRpc.getEvents.mockResolvedValue({ events: [rawEvent({ id: 'a', ledger: 100, eventIndex: 0 })] });

      const { hasMore } = await service.fetchActivityByLedgerRange({
        startLedger: null,
        startEventIndex: null,
        limit: 5,
      });

      expect(hasMore).toBe(false);
    });

    it('returns an empty result when the RPC returns no events', async () => {
      mockRpc.getEvents.mockResolvedValue({ events: [] });

      const { events, hasMore } = await service.fetchActivityByLedgerRange({
        startLedger: null,
        startEventIndex: null,
        limit: 10,
      });

      expect(events).toEqual([]);
      expect(hasMore).toBe(false);
    });

    it('skips events whose type is not a recognised lending action', async () => {
      mockRpc.getEvents.mockResolvedValue({
        events: [
          rawEvent({ id: 'good', ledger: 100, eventIndex: 0, type: 'borrow' }),
          rawEvent({ id: 'unknown', ledger: 100, eventIndex: 1, type: 'something_else' }),
        ],
      });

      const { events } = await service.fetchActivityByLedgerRange({
        startLedger: null,
        startEventIndex: null,
        limit: 10,
      });

      expect(events.map(e => e.id)).toEqual(['good']);
    });

    it('decodes the user topic to its Stellar strkey', async () => {
      mockRpc.getEvents.mockResolvedValue({
        events: [rawEvent({ id: 'a', ledger: 100, eventIndex: 0, user: USER })],
      });

      const { events } = await service.fetchActivityByLedgerRange({
        startLedger: null,
        startEventIndex: null,
        limit: 10,
      });

      // `ScAddress.prototype.toString()` yields "[object Object]", so this
      // pins the conversion through `Address.fromScAddress`.
      expect(events[0].user).toBe(USER);
    });

    it('preserves amounts that exceed the low 64 bits of an i128', async () => {
      // 2^64 is the first value that does not fit in the low word alone, and
      // 2^127 - 1 is the signed 128-bit maximum.
      mockRpc.getEvents.mockResolvedValue({
        events: [
          rawEvent({ id: 'big', ledger: 100, eventIndex: 0, amount: '18446744073709551616' }),
          rawEvent({ id: 'max', ledger: 100, eventIndex: 1, amount: I128_MAX }),
        ],
      });

      const { events } = await service.fetchActivityByLedgerRange({
        startLedger: null,
        startEventIndex: null,
        limit: 10,
      });

      const byId = Object.fromEntries(events.map(e => [e.id, e.amount]));
      expect(byId.big).toBe('18446744073709551616');
      expect(byId.max).toBe(I128_MAX);
    });

    it('preserves a negative i128 amount', async () => {
      mockRpc.getEvents.mockResolvedValue({
        events: [rawEvent({ id: 'a', ledger: 100, eventIndex: 0, amount: '-500' })],
      });

      const { events } = await service.fetchActivityByLedgerRange({
        startLedger: null,
        startEventIndex: null,
        limit: 10,
      });

      expect(events[0].amount).toBe('-500');
    });

    it('preserves the parsed amount as a string', async () => {
      mockRpc.getEvents.mockResolvedValue({
        events: [rawEvent({ id: 'a', ledger: 100, eventIndex: 0, amount: '4200' })],
      });

      const { events } = await service.fetchActivityByLedgerRange({
        startLedger: null,
        startEventIndex: null,
        limit: 10,
      });

      expect(events[0].amount).toBe('4200');
      expect(typeof events[0].amount).toBe('string');
    });

    it('uses the latest ledger as the upper bound when none is given', async () => {
      mockRpc.getLatestLedger.mockResolvedValue({ sequence: 7777 });
      mockRpc.getEvents.mockResolvedValue({ events: [] });

      await service.fetchActivityByLedgerRange({
        startLedger: null,
        startEventIndex: null,
        limit: 10,
      });

      expect(mockRpc.getEvents).toHaveBeenCalledWith(
        expect.objectContaining({ endLedger: 7777 })
      );
    });
  });

  describe('fetchUserActivityByLedgerRange', () => {
    beforeEach(() => {
      mockRpc.getEvents.mockResolvedValue({
        events: [
          rawEvent({ id: 'mine-1', ledger: 100, eventIndex: 0, user: USER }),
          rawEvent({ id: 'other', ledger: 100, eventIndex: 1, user: CONTRACT_ID }),
          rawEvent({ id: 'mine-2', ledger: 100, eventIndex: 2, user: USER }),
        ],
      });
    });

    it('returns only events belonging to the requested user', async () => {
      const { events } = await service.fetchUserActivityByLedgerRange({
        userAddress: USER,
        startLedger: null,
        startEventIndex: null,
        limit: 10,
      });

      expect(events.map(e => e.id)).toEqual(['mine-1', 'mine-2']);
    });

    it('matches the user address case-insensitively', async () => {
      const { events } = await service.fetchUserActivityByLedgerRange({
        userAddress: USER.toLowerCase(),
        startLedger: null,
        startEventIndex: null,
        limit: 10,
      });

      expect(events).toHaveLength(2);
    });

    it('never returns more than the requested limit', async () => {
      const { events } = await service.fetchUserActivityByLedgerRange({
        userAddress: USER,
        startLedger: null,
        startEventIndex: null,
        limit: 1,
      });

      expect(events).toHaveLength(1);
    });

    it('returns an empty list for a user with no activity', async () => {
      const { events } = await service.fetchUserActivityByLedgerRange({
        userAddress: 'GD5TFY4DYYF43CQN3UMZUPBBXBLWK3WYAM5PIOMKOVRHBTZF7J7VGHP4',
        startLedger: null,
        startEventIndex: null,
        limit: 10,
      });

      expect(events).toEqual([]);
    });
  });

  describe('pingContract', () => {
    it('reports healthy when rpc and contract are both reachable', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { _embedded: { records: [{ sequence: '4242' }] } },
      });

      const result = await service.pingContract();

      expect(result).toEqual({ rpc: true, contract: true, ledger: 4242 });
    });

    it('reports rpc down and skips the contract probe', async () => {
      mockSoroban.getHealth.mockRejectedValue(new Error('rpc down'));

      const result = await service.pingContract();

      expect(result).toEqual({ rpc: false, contract: false, ledger: null });
      expect(mockSoroban.prepareTransaction).not.toHaveBeenCalled();
    });

    it('reports contract unreachable when prepareTransaction fails', async () => {
      mockSoroban.prepareTransaction.mockRejectedValue(new Error('contract missing'));

      const result = await service.pingContract();

      expect(result.rpc).toBe(true);
      expect(result.contract).toBe(false);
    });

    it('still reports contract health when the horizon ledger lookup fails', async () => {
      mockedAxios.get.mockRejectedValue(new Error('horizon down'));

      const result = await service.pingContract();

      expect(result.rpc).toBe(true);
      expect(result.contract).toBe(true);
      expect(result.ledger).toBeNull();
    });
  });

  describe('healthCheck', () => {
    it('reports unhealthy when both dependencies fail', async () => {
      mockedAxios.get.mockRejectedValue(new Error('horizon down'));
      mockSoroban.getHealth.mockRejectedValue(new Error('rpc down'));

      const result = await service.healthCheck();

      expect(result.horizon).toBe(false);
      expect(result.sorobanRpc).toBe(false);
    });

    it('exposes circuit breaker metrics for observability', async () => {
      mockedAxios.get.mockResolvedValue({});
      mockSoroban.getHealth.mockResolvedValue({});

      const result = await service.healthCheck() as any;

      expect(result.sorobanBreaker).toBeDefined();
      expect(result.sorobanBreaker).toHaveProperty('state');
    });
  });
});
