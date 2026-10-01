/**
 * Guard and failure-path coverage for the Stellar event parsers and the
 * circuit-breaker interaction in `healthCheck`.
 *
 * These branches are the ones that decide what the API returns when a Soroban
 * event is malformed, so they are exercised here against real ScVal/XDR
 * objects rather than hand-rolled stand-ins.
 */

import { Address, nativeToScVal, xdr } from '@stellar/stellar-sdk';
import axios from 'axios';
import { StellarService } from '../stellar.service';
import { AMM_EVENT_TOPIC_MODULE, AMM_EVENT_TOPIC_VERSION } from '../../types';

jest.mock('axios');
const mockedAxios = axios as jest.Mocked<typeof axios>;

const CONTRACT_ID = 'CADQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQOBYHA4DQP5KR';
const USER = 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK';

process.env.CONTRACT_ID = CONTRACT_ID;
const { StellarService: Service } = require('../stellar.service') as {
  StellarService: new (rpcUrl?: string, lendingContractId?: string) => StellarService;
};

describe('StellarService event parsing guards', () => {
  let service: StellarService;

  beforeEach(() => {
    jest.clearAllMocks();
    service = new Service('https://rpc.test', CONTRACT_ID);
  });

  const parse = (fn: string, ...args: unknown[]) =>
    (service as unknown as Record<string, (...a: unknown[]) => unknown>)[fn](...args);

  describe('parseEventType', () => {
    it('returns null for a value that is not a symbol', () => {
      expect(parse('parseEventType', xdr.ScVal.scvU32(1))).toBeNull();
    });

    it('returns null for a non-lending event name', () => {
      expect(parse('parseEventType', xdr.ScVal.scvSymbol('transfer'))).toBeNull();
    });

    it('returns the lending event name', () => {
      expect(parse('parseEventType', xdr.ScVal.scvSymbol('borrow'))).toBe('borrow');
    });
  });

  describe('parseAddress', () => {
    it('returns an empty string when the topic is absent', () => {
      expect(parse('parseAddress', undefined)).toBe('');
    });

    it('returns an empty string for a value that is not an address', () => {
      expect(parse('parseAddress', xdr.ScVal.scvSymbol('not-an-address'))).toBe('');
    });

    it('decodes a valid address topic', () => {
      const val = xdr.ScVal.scvAddress(Address.fromString(USER).toScAddress());
      expect(parse('parseAddress', val)).toBe(USER);
    });
  });

  describe('parseEventValue', () => {
    it('falls back to zero when the value is not a map', () => {
      expect(parse('parseEventValue', xdr.ScVal.scvU32(7))).toEqual({ amount: '0', asset: '' });
    });

    it('reads amount and asset from a map', () => {
      const value = xdr.ScVal.scvMap([
        new xdr.ScMapEntry({
          key: xdr.ScVal.scvSymbol('amount'),
          val: nativeToScVal('4242', { type: 'i128' }),
        }),
        new xdr.ScMapEntry({
          key: xdr.ScVal.scvSymbol('asset'),
          val: xdr.ScVal.scvAddress(Address.fromString(USER).toScAddress()),
        }),
      ]);

      expect(parse('parseEventValue', value)).toEqual({ amount: '4242', asset: USER });
    });

    it('falls back to zero when a map entry is the wrong type', () => {
      const value = xdr.ScVal.scvMap([
        new xdr.ScMapEntry({
          key: xdr.ScVal.scvSymbol('amount'),
          val: xdr.ScVal.scvString('not-an-i128'),
        }),
      ]);

      expect(parse('parseEventValue', value)).toEqual({ amount: '0', asset: '' });
    });
  });

  describe('parseSingleEvent', () => {
    const base = {
      id: 'evt',
      ledgerClosedAt: '2026-01-01T00:00:00Z',
      txHash: 'tx',
      inSuccessfulContractCall: true,
    };

    it('returns null when the event has no topics at all', () => {
      expect(
        parse('parseSingleEvent', { ...base, topic: [] }, 100, 0)
      ).toBeNull();
    });

    it('returns null when the value is not a map', () => {
      const topic = [
        xdr.ScVal.scvSymbol('deposit'),
        xdr.ScVal.scvAddress(Address.fromString(USER).toScAddress()),
      ];

      const result = parse('parseSingleEvent', { ...base, topic, value: xdr.ScVal.scvU32(1) }, 100, 0);

      // The event is still usable: a bad value degrades to a zero amount
      // rather than dropping the event.
      expect(result).toEqual(expect.objectContaining({ user: USER, amount: '0', asset: '' }));
    });
  });

  describe('parseAmmEventTopic', () => {
    it('rejects a non-array topic', () => {
      expect(parse('parseAmmEventTopic', 'nope')).toBeNull();
    });

    it('rejects the wrong tuple length', () => {
      expect(parse('parseAmmEventTopic', [AMM_EVENT_TOPIC_MODULE, AMM_EVENT_TOPIC_VERSION])).toBeNull();
    });

    it('rejects an unknown module', () => {
      expect(
        parse('parseAmmEventTopic', ['other', AMM_EVENT_TOPIC_VERSION, 'swap'])
      ).toBeNull();
    });

    it('rejects an unknown version', () => {
      expect(parse('parseAmmEventTopic', [AMM_EVENT_TOPIC_MODULE, 'v2', 'swap'])).toBeNull();
    });

    it('rejects an unknown event kind', () => {
      expect(
        parse('parseAmmEventTopic', [AMM_EVENT_TOPIC_MODULE, AMM_EVENT_TOPIC_VERSION, 'flash_swap'])
      ).toBeNull();
    });

    it('accepts a well-formed topic', () => {
      expect(
        parse('parseAmmEventTopic', [AMM_EVENT_TOPIC_MODULE, AMM_EVENT_TOPIC_VERSION, 'swap'])
      ).toEqual({ module: AMM_EVENT_TOPIC_MODULE, version: AMM_EVENT_TOPIC_VERSION, kind: 'swap' });
    });
  });

  describe('decodeAmmEvent', () => {
    const topics = [AMM_EVENT_TOPIC_MODULE, AMM_EVENT_TOPIC_VERSION, 'swap'];

    it('rejects a non-object payload', () => {
      expect(parse('decodeAmmEvent', null)).toBeNull();
      expect(parse('decodeAmmEvent', 'string')).toBeNull();
    });

    it('rejects a payload with no data', () => {
      expect(parse('decodeAmmEvent', { topics })).toBeNull();
    });

    it('rejects a non-object data field', () => {
      expect(parse('decodeAmmEvent', { topics, data: 'oops' })).toBeNull();
    });

    it('rejects an unsupported schema version', () => {
      expect(
        parse('decodeAmmEvent', { topics, data: { schema_version: 2, event: 'swap' } })
      ).toBeNull();
    });

    it('rejects a data event that disagrees with the topic kind', () => {
      expect(
        parse('decodeAmmEvent', { topics, data: { schema_version: 1, event: 'add_liquidity' } })
      ).toBeNull();
    });

    it('decodes a matching payload', () => {
      expect(
        parse('decodeAmmEvent', { topics, data: { schema_version: 1, event: 'swap' } })
      ).toEqual({ topic: { module: AMM_EVENT_TOPIC_MODULE, version: AMM_EVENT_TOPIC_VERSION, kind: 'swap' }, data: { schema_version: 1, event: 'swap' } });
    });
  });

  describe('extractAmmEventsFromTransactionResult', () => {
    it('returns an empty list for a missing result', () => {
      expect(parse('extractAmmEventsFromTransactionResult', null)).toEqual([]);
    });

    it('returns an empty list when events is not an array', () => {
      expect(parse('extractAmmEventsFromTransactionResult', { events: 'nope' })).toEqual([]);
    });

    it('keeps only the decodable events', () => {
      const topics = [AMM_EVENT_TOPIC_MODULE, AMM_EVENT_TOPIC_VERSION, 'swap'];
      const result = parse(
        'extractAmmEventsFromTransactionResult',
        {
          events: [
            { topics, data: { schema_version: 1, event: 'swap' } },
            { topics: ['bad'] },
            null,
          ],
        }
      );

      expect(result).toHaveLength(1);
    });
  });

  describe('healthCheck with an open circuit breaker', () => {
    it('reports the RPC unhealthy without calling it when the breaker is open', async () => {
      mockedAxios.get.mockResolvedValue({});

      const breaker = (service as unknown as { sorobanBreaker: { record(s: boolean): void; getState(): string } })
        .sorobanBreaker;
      // Drive the breaker to OPEN: it needs `minRequests` (5) samples before it
      // evaluates the failure rate.
      for (let i = 0; i < 5; i++) {
        breaker.record(false);
      }
      expect(breaker.getState()).toBe('OPEN');

      const result = await service.healthCheck();

      expect(result.horizon).toBe(true);
      expect(result.sorobanRpc).toBe(false);
      expect((result as unknown as { sorobanBreaker: { state: string } }).sorobanBreaker.state).toBe('OPEN');
    });
  });
});
