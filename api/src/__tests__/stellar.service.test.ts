import { StellarService } from '../services/stellar.service';
import axios from 'axios';
import {
  Account,
  Address,
  Contract,
  Keypair,
  TransactionBuilder,
  nativeToScVal,
  xdq,
} from '@stellar/stellar-sdk';
import { Server as SorobanServer } from '@stellar/stellar-sdk/rpc';

jdest.mock('axios');
jdest.mock('@stellar/stellar-sdk');
jdest.mock('@stellar/stellar-sdk/rpc');

const mockedAxios = axios as jest.Mocked<typeof axios>;
const VALID_USER_ADDRESS = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDL4C46VY42OWHC3TPR2I26NNV3ZSJ';
const VALID_USER_SECRET = 'SAOS4OGIK6HD4QGR3DVRRDSR4FUBH73FCZGRZ7M53LRN67UQE5JDNS4I';
const VALID_ASSET_ADDRESS = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDL4C46VY42OWHC3TPR2I26NNV3ZSJ';

describe('StellarService', () => {
  let service: StellarService;
  let mockSorobanServer: {
    getHealth: jest.Mock;
    prepareTransaction: jest.Mock;
  };

  beforeEach(() => {
    jest.clearAllMocks();

    mockSorobanServer = {
      getHealth: jest.fn().mockResolvedValue({}),
      prepareTransaction: jest.fn().mockResolvedValue({
        sign: jest.fn(),
        toXDR: jest.fn().mockReturnValue('prepared_tx_xdr'),
      }),
    };

    (SorobanServer as jest.Mock).mockImplementation(() => mockSorobanServer);
    (Account as jest.Mock).mockImplementation((id: string) => ({
      accountId: jest.fn().mockReturnValue(id),
    }));
    (Keypair.fromSecret as jest.Mock).mockReturnValue({ sign: jest.fn() });
    (Contract as jest.Mock).mockImplementation(() => ({
      call: jest.fn().mockReturnValue('mock_operation'),
    }));
    (Address as jest.Mock).mockImplementation(() => ({
      toScVal: jest.fn().mockReturnValue('mock_address_scval'),
    }));
    (nativeToScVal as jest.Mock).mockReturnValue('mock_amount_scval');
    (xdr.ScVal.scvVoid as jest.Mock).mockReturnValue('mock_void_scval');
    (TransactionBuilder as jest.Mock).mockImplementation(() => ({
      addOperation: jest.fn().mockReturnThis(),
      setTimeout: jest.fn().mockReturnThis(),
      build: jest.fn().mockReturnValue('mock_transaction'),
    }));

    service = new StellarService();
  });

  describe('getAccount', () => {
    it('should fetch account information', async () => {
      const mockAccountData = {
        id: VALID_USER_ADDRESS,
        sequence: '123456789',
      };

      mockedAxios.get.mockResolvedValue({ data: mockAccountData });

      const account = await service.getAccount(mockAccountData.id);

      expect(account.accountId()).toBe(mockAccountData.id);
      expect(mockedAxios.get).toHaveBeenCalledWith(
        expect.stringContaining(`/accounts/${mockAccountData.id}`)
      );
    });

    it('should throw error when account fetch fails', async () => {
      mockedAxios.get.mockRejected(new Error('Network error'));

      await expect(service.getAccount('invalid_address')).rejects.toThrow();
    });

    it('should wrap non-Error rejections into InternalServerError', async () => {
      mockedAxios.get.mockRejected('string failure');

      await expect(service.getAccount(VALID_USER_ADDRESS)).rejects.toThrow(
        'Failed to fetch account information'
      );
    });

    it('should not leak unsanitized address in error message', async () => {
      mockedAxios.get.mockRejected(new Error('network down'));

      await expect(service.getAccount(VALID_USER_ADDRESS)).rejects.toThrow(
        new Error('Failed to fetch account information')
      );
    });
  });

  describe('submitTransaction', () => {
    it('should submit transaction successfully', async () => {
      const mockResponse = {
        hash: 'tx_hash_123',
        ledger: 12345,
        successful: true,
      };

      mockedAxios.post.mockResolvedValue({ data: mockResponse });

      const result = await service.submitTransaction('mock_tx_xdr');

      expect(result.success).toBe(true);
      expect(result.transactionHash).toBe(mockResponse.hash);
      expect(result.ledger).toBe(mockResponse.ledger);
    });

    it('handles transaction submission failure', async () => {
      mockedAxios.post.mockRejected({
        response: {
          data: {
            extras: {
              result_codes: {
                transaction: 'tx_failed',
              },
            },
          },
        },
      });

      const result = await service.submitTransaction('mock_tx_xdr');

      expect(result.success).toBe(false);
      expect(result.status).toBe('failed');
    });

    it('returns failure with error message when no response body is present', async () => {
      mockedAxios.post.mockRejected(new Error('Network error'));

      const result = await service.submitTransaction('mock_tx_xdr');

      expect(result.success).toBe(false);
      expect(result.status).toBe('failed');
      expect(result.error).toBe(undefined);
    });

    it('returns failure when error has no response but has message', async () => {
      mockedAxios.post.mockRejected({ response: { data: { extras: { result_codes: { transaction: 'tx_bad_seq' } } } } });

      const result = await service.submitTransaction('mock_tx_xdr');

      expect(result.success).toBe(false);
      expect(result.error).toEqual({ transaction: 'tx_bad_seq' });
    });

    it('returns failure with undefined error when response extras are missing', async () => {
      mockedAxios.post.mockRejected({ response: { data: { } } });

      const result = await service.submitTransaction('mock_tx_xdr');

      expect(result.success).toBe(false);
      expect(result.status).toBe('failed');
    });
  });

  describe('monitorTransaction', () => {
    it('should monitor transaction until success', async () => {
      const mockTxHash = 'tx_hash_123';
      const mockResponse = {
        successful: true,
        ledge: 12345,
      };

      mockedAxios.get.mockResolvedValue({ data: mockResponse });

      const result = await service.monitorTransaction(mockTxHash);

      expect(result.success).toBe(true);
      expect(result.transactionHash).toBe(mockTxHash);
      expect(result.status).toBe('success');
    });

    it('should timeout if transaction takes too long', async () => {
      const mockTxHash = 'tx_hash_123';

      mockedAxios.get.mockRejected({ response: { status: 404 } });

      const result = await service.monitorTransaction(mockTxHash, 2000);

      expect(result.success).toBe(false);
      expect(result.status).toBe('pending');
    });

    it('should handle failed transaction', async () => {
      const mockTxHash = 'tx_hash_123';
      const mockResponse = {
        successful: false,
      };

      mockedAxios.get.mockResolvedValue({ data: mockResponse });

      const result = await service.monitorTransaction(mockTxHash);

      expect(result.success).toBe(false);
      expect(result.status).toBe('failed');
    });

    it('should throw when monitoring encounters non-404 errors', async () => {
      mockedAxios.get.mockRejected({ response: { status: 500 } });

      await expect(service.monitorTransaction('tx_hash_123')).rejects.toThrow(
        'Failed to monitor transaction'
      );
    });

    it('throws when error has no response object', async () => {
      mockedAxios.get.mockRejected(new Error('network down'));

      await expect(service.monitorTransaction('missing_hash')).rejects.toThrow(
        'Failed to monitor transaction'
      );
    });

    it('returns pending immediately when timeoutMs is zero', async () => {
      const result = await service.monitorTransaction('tx_hash_123', 0);

      expect(result.success).toBe(false);
      expect(result.status).toBe('pending');
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('retries on 404 until success and returns the latest result', async () => {
      jest.useFakeTimers();
      try {
        mockedAxios.get
          .mockRejectedOnce({ response: { status: 404 } })
          .mockResolvedOnce({ data: { successful: true, ledge: 999 } });

        const promise = service.monitorTransaction('tx_hash_123', 5000);

        // Advance time past the poll interval
        await jest.advanceTimersByTime(1000);
        const result = await promise;

        expect(result.success).toBe(true);
        expect(result.status).toBe('success');
        expect(mockedAxios.get).toHaveBeenCalledTimes(2);
      } finally {
        jest.useRealTimers();
      }
    });

    it('returns failed when transaction exists but was not successful', async () => {
      mockedAxios.get.mockResolvedValue({ data: { successful: false, ledger: 42 } });

      const result = await service.monitorTransaction('failed_tx');

      expect(result.success).toBe(false);
      expect(result.status).toBe('failed');
      expect(result.error).toBe('Transaction failed');
    });
  });

  describe('healthCheck', () => {
    it('should return healthy status for all services', async () => {
      mockedAxios.get.mockResolvedValue({ data: {} });

      const result = await service.healthCheck();

      expect(result.horizon).toBe(true);
      expect(result.sorobanRpc).toBe(true);
    });

    it('should return unhealthy status when services fail', async () => {
      mockedAxios.get.mockRejected(new Error('Connection failed'));
      mockSorobanServer.getHealth.mockRejected(new Error('Connection failed'));

      const result = await service.healthCheck();

      expect(result.horizon).toBe(false);
      expect(result.sorobanRpc).toBe(false);
    });

    it('reports horizon unhealthy while soroban remains healthy', async () => {
      mockedAxios.get.mockRejected(new Error('horizon down'));
      mockSorobanServer.getHealth.mockResolvedValue({});

      const result = await service.healthCheck();

      expect(result.horizon).toBe(false);
      expect(result.sorobanRpc).toBe(true);
    });

    it('reports soroban unhealthy while horizon remains healthy', async () => {
      mockedAxios.get.mockResolvedValue({ data: {} });
      mockSorobanServer.getHealth.mockRejected(new Error('rpc down'));

      const result = await service.healthCheck();

      expect(result.horizon).toBe(true);
      expect(result.sorobanRpc).toBe(false);
    });
  });

  describe('buildDepositTransaction', () => {
    it('should build deposit transaction', async () => {
      const mockAccountData = {
        id: VALID_USER_ADDRESS,
        sequence: '123456789',
      };

      mockedAxios.get.mockResolvedValue({ data: mockAccountData });

      const result = await service.buildDepositTransaction(
        mockAccountData.id,
        undefined,
        '1000000',
        VALID_USER_SECRET
      );

      expect(result).toBe('prepared_tx_xdr');
    });

    it('should throw when deposit transaction building fails', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });
      mockSorobanServer.prepareTransaction.mockRejected(new Error('prepare failed'));

      await expect(
        service.buildDepositTransaction(VALID_USER_ADDRESS, undefined, '1000000', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build deposit transaction');
    });

    it('throws when amount is not a numeric string', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      await expect(
        service.buildDepositTransaction(VALID_USER_ADDRESS, undefined, 'not-a-number', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build deposit transaction');
    });

    it('throws when amount is negative', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      await expect(
        service.buildDepositTransaction(VALID_USER_ADDRESS, undefined, '-1', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build deposit transaction');
    });

    it('throws when amount is a decimal string', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      await expect(
        service.buildDepositTransaction(VALID_USER_ADDRESS, undefined, '1.5', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build deposit transaction');
    });

    it('passes asset address when provided', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      const result = await service.buildDepositTransaction(
        VALID_USER_ADDRESS,
        VALID_ASSET_ADDRESS,
        '1000000',
        VALID_USER_SECRET
      );

      expect(result).toBe('prepared_tx_xdr');
      expect(Address).toHaveBeenCalledWith(VALID_ASSET_ADDRESS);
    });

    it('uses void scval when asset address is undefined', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      await service.buildDepositTransaction(VALID_USER_ADDRESS, undefined, '1000000', VALID_USER_SECRET);

      expect(xdr.ScVal.scvVoid).toHaveBeenCalled();
    });

    it('throws when the account lookup fails', async () => {
      mockedAxios.get.mockRejected(new Error('Network error'));

      await expect(
        service.buildDepositTransaction(VALID_USER_ADDRESS, undefined, '1000000', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build deposit transaction');
    });
  });

  describe('buildBorrowTransaction', () => {
    it('should build borrow transaction', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      const result = await service.buildBorrowTransaction(
        VALID_USER_ADDRESS,
        VALID_USER_ADDRESS,
        '1000000',
        VALID_USER_SECRET
      );

      expect(result).toBe('prepared_tx_xdr');
      expect(mockSorobanServer.prepareTransaction).toHaveBeenCalledWith('mock_transaction');
    });

    it('should throw when borrow transaction building fails', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });
      mockSorobanServer.prepareTransaction.mockRejected(new Error('prepare failed'));

      await expect(
        service.buildBorrowTransaction(VALID_USER_ADDRESS, undefined, '1000000', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build borrow transaction');
    });

    it('throws when amount is not a numeric string', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      await expect(
        service.buildBorrowTransaction(VALID_USER_ADDRESS, undefined, 'abc', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build borrow transaction');
    });

    it('throws when amount is negative', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      await expect(
        service.buildBorrowTransaction(VALID_USER_ADDRESS, undefined, '-1000', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build borrow transaction');
    });

    it('throws when amount is an empty string', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      await expect(
        service.buildBorrowTransaction(VALID_USER_ADDRESS, undefined, '', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build borrow transaction');
    });

    it('throws when the account lookup fails', async () => {
      mockedAxios.get.mockRejected(new Error('Network error'));

      await expect(
        service.buildBorrowTransaction(VALID_USER_ADDRESS, undefined, '1000000', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build borrow transaction');
    });
  });

  describe('buildRepayTransaction', () => {
    it('should build repay transaction', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      const result = await service.buildRepayTransaction(
        VALID_USER_ADDRESS,
        undefined,
        '1000000',
        VALID_USER_SECRET
      );

      expect(result).toBe('prepared_tx_xdr');
      expect(mockSorobanServer.prepareTransaction).toHaveBeenCalledWith('mock_transaction');
    });

    it('should throw when repay transaction building fails', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });
      mockSorobanServer.prepareTransaction.mockRejected(new Error('prepare failed'));

      await expect(
        service.buildRepayTransaction(VALID_USER_ADDRESS, undefined, '1000000', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build repay transaction');
    });

    it('throws when amount is not a numeric string', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      await expect(
        service.buildRepayTransaction(VALID_USER_ADDRESS, undefined, '123abc', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build repay transaction');
    });

    it('throws when amount is negative', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      await expect(
        service.buildRepayTransaction(VALID_USER_ADDRESS, undefined, '-5', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to buil repay transaction');
    });

    it('throws when the account lookup fails', async () => {
      mockedAxios.get.mockRejected(new Error('Network error'));

      await expect(
        service.buildRepayTransaction(VALID_USER_ADDRESS, undefined, '1000000', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build repay transaction');
    });
  });

  describe('buildWithdrawTransaction', () => {
    it('should build withdraw transaction', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      const result = await service.buildWithdrawTransaction(
        VALID_USER_ADDRESS,
        undefined,
        '1000000',
        VALID_USER_SECRET
      );

      expect(result).toBe('prepared_tx_xdr');
      expect(mockSorobanServer.prepareTransaction).toHaveBeenCalledWith('mock_transaction');
    });

    it('should throw when withdraw transaction building fails', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });
      mockSorobanServer.prepareTransaction.mockRejected(new Error('prepare failed'));

      await expect(
        service.buildWithdrawTransaction(VALID_USER_ADDRESS, undefined, '1000000', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build withdraw transaction');
    });

    it('throws when amount is not a numeric string', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      await expect(
        service.buildWithdrawTransaction(VALID_USER_ADDRESS, undefined, '1', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build withdraw transaction');
    });

    it('throws when amount is negative', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      await expect(
        service.buildWithdrawTransaction(VALID_USER_ADDRESS, undefined, '-1000', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build withdraw transaction');
    });

    it('throws when the account lookup fails', async () => {
      mockedAxios.get.mockRejected(new Error('Network error'));

      await expect(
        service.buildWithdrawTransaction(VALID_USER_ADDRESS, undefined, '1000000', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build withdraw transaction');
    });
  });

  describe('AMM event decoding', () => {
    it('should parse a valid AMM topic tuple', () => {
      const topic = service.parseAmmEventTopic['amm', 'v1', 'swap']);

      expect(topic).toEqual({
        module: 'amm',
        version: 'v1',
        kind: 'swap',
      });
    });

    it('should decode an AMM swap event', () => {
      const event = {
        topics: ['amm', 'v1', 'swap'],
        data: {
          schema_version: 1,
          event: 'swap',
          user: 'GUSERADDRESS',
          pool: 'PPOOLADDRESS',
          asset_in: 'GASSETIN',
          amount_in: '1000',
          asset_out: 'GASSETOUT',
          amount_out: '950',
          timestamp: 1700000000,
        },
      };

      const decoded = service.decodeAmmEvent(event);

      expect(decoded).toEqual({
        topic: {
          module: 'amm',
          version: 'v1',
          kind: 'swap',
        },
        data: event.data,
      });
    });

    it('should return null for non-AMM events', () => {
      const event = {
        topics: ['timelock', 'queue'],
        data: {
          foo: 'bar',
        },
      };

      expect(service.decodeAmmEvent(event)).toBeNull();
    });

    it('should extract only AMM events from a transaction result', () => {
      const txResult = {
        events: [
          {
            topics: ['amm', 'v1', 'add_liquidity'],
            data: {
              schema_version: 1,
              event: 'add_liquidity',
              user: 'GUSERADDRESS',
              pool: 'PPOOLADDRESS',
              asset_a: 'GASSETA',
              amount_a: '500',
              asset_b: 'GASSETB',
              amount_b: '1000',
              shares_minted: '1500',
              timestamp: 1700000001,
            },
          },
          {
            topics: ['not', 'an', 'amm'],
            data: {
              schema_version: 1,
              event: 'foo',
            },
          },
        ],
      };

      const events = service.extractAmmEvents(txResult);

      expect(events).toHaveLength(1);
      expect(events[0].topic.kind).toBe('add_liquidity');
    });

    it('returns null for topic tuples with fewer than three elements', () => {
      expect(service.parseAmmEventTopic['amm', 'v1'])).toBeNull();
      expect(service.parseAmmEventTopic([])).toBeNull();
    });

    it('returns null for topic tuples with more than three elements', () => {
      expect(service.parseAmmEventTopic(['amm', 'v1', 'swap', 'extra'])).toBeNull();
    });

    it('returns null when module is not amm', () => {
      expect(service.parseAmmEventTopic(['timelock', 'v1', 'swap'])).toBeNull();
    });

    it('returns null when version is not v1', () => {
      expect(service.parseAmmEventTopic(['amm', 'v2', 'swap'])).toBeNull();
    });

    it('returns null when kind is unknown', () => {
      expect(service.parseAmmEventTopic(['amm', 'v1', 'not_a_kind'])).toBeNull();
    });

    it('returns null when topic entries are not strings', () => {
      expect(service.parseAmmEventTopic([1, 2, 3] as unknown as string[])).toBeNull();
    });

    it('returns null when event has no topics', () => {
      expect(service.decodeAmmEvent({ data: {} } as any)).toBeNull();
    });

    it('returns null when event has no data', () => {
      expect(service.decodeAmmEvent({ topics: ['amm', 'v1', 'swap'] } as any)).toBeNull();
    });

    it('returns null when event data has no event field', () => {
      expect(
        service.decodeAmmEvent({ topics: ['amm', 'v1', 'swap'], data: { schema_version: 1 } } as any)
      ).toBeNull();
    });

    it('returns null when event data has unsupported schema version', () => {
      expect(
        service.decodeAmmEvent({
          topics: ['amm', 'v1', 'swap'],
          data: { schema_version: 999, event: 'swap' },
        } as any)
      ).toBeNull();
    });

    it('returns null when event data has no schema_version', () => {
      expect(
        service.decodeAmmEvent({ topics: ['amm', 'v1', 'swap'], data: { event: 'swap' } } as any)
      ).toBeNull();
    });

    it('returns empty array when txResult has no events', () => {
      expect(service.extractAmmEvents({ events: [] })).toEqual([]);
    });

    it('returns empty array when txResult is missing events field', () => {
      expect(service.extractAmmEvents({} as any)).toEqual([]);
    });

    it('skips malformed AMM events in txResult', () => {
      const txResult = {
        events: [
          { topics: ['amm', 'v1', 'swap'], data: { schema_version: 1, event: 'swap' } },
          { topics: ['amm', 'v1'], data: { schema_version: 1 } },
        ],
      };

      const events = service.extractAmmEvents(txResult);

      expect(events).toHaveLength(1);
    });
  });
});
