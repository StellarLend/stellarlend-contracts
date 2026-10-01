import { StellarService } from '../services/stellar.service';
import axios from 'axios';
import {
  Account,
  Address,
  Contract,
  Keypair,
  TransactionBuilder,
  nativeToScVal,
  xdr,
} from '@stellar/stellar-sdk';
import { Server as SorobanServer } from '@stellar/stellar-sdk/rpc';

jest.mock('axios');
jest.mock('@stellar/stellar-sdk');
jest.mock('@stellar/stellar-sdk/rpc');

const mockedAxios = axios as jest.Mocked<typeof axios>;
const VALID_USER_ADDRESS = 'GBLXVKWHD4QAPFLHMJDXSVB6GFUDLTC46VY42OWHC3TPRN2I6NNV3ZSJ';
const VALID_USER_SECRET = 'SAOS4OGIK6HD4QGR3DVRRDSR4FUBH73FCZGRZ7M53LRN67UQE5JDNS4I';
const VALID_CONTRACT_ID = 'CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSC4';

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
      mockedAxios.get.mockRejectedValue(new Error('Network error'));

      await expect(service.getAccount('invalid_address')).rejects.toThrow();
    });

    it('should reject an empty account id before hitting the network', async () => {
      await expect(service.getAccount('')).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should reject a malformed account id before hitting the network', async () => {
      await expect(service.getAccount('not-a-stellar-address')).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should reject a non-string account id', async () => {
      await expect(service.getAccount(undefined as unknown as string)).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
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

    it('should handle transaction submission failure', async () => {
      mockedAxios.post.mockRejectedValue({
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

    it('should reject an empty transaction envelope before submitting', async () => {
      await expect(service.submitTransaction('')).rejects.toThrow();
      expect(mockedAxios.post).not.toHaveBeenCalled();
    });

    it('should reject a non-string transaction envelope', async () => {
      await expect(
        service.submitTransaction(undefined as unknown as string)
      ).rejects.toThrow();
      expect(mockedAxios.post).not.toHaveBeenCalled();
    });

    it('should not leak sensitive error details in the failure result', async () => {
      mockedAxios.post.mockRejectedValue({
        response: {
          data: {
            extras: {
              result_codes: { transaction: 'tx_failed' },
              envelope_xdr: 'SECRET_ENVELOPE_XDR',
            },
          },
        },
      });

      const result = await service.submitTransaction('mock_tx_xdr');

      expect(result.success).toBe(false);
      expect(JSON.stringify(result)).not.toContain('SECRET_ENVELOPE_XDR');
    });
  });

  describe('monitorTransaction', () => {
    it('should monitor transaction until success', async () => {
      const mockTxHash = 'tx_hash_123';
      const mockResponse = {
        successful: true,
        ledger: 12345,
      };

      mockedAxios.get.mockResolvedValue({ data: mockResponse });

      const result = await service.monitorTransaction(mockTxHash);

      expect(result.success).toBe(true);
      expect(result.transactionHash).toBe(mockTxHash);
      expect(result.status).toBe('success');
    });

    it('should timeout if transaction takes too long', async () => {
      const mockTxHash = 'tx_hash_123';

      mockedAxios.get.mockRejectedValue({ response: { status: 404 } });

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
      mockedAxios.get.mockRejectedValue({ response: { status: 500 } });

      await expect(service.monitorTransaction('tx_hash_123')).rejects.toThrow(
        'Failed to monitor transaction'
      );
    });

    it('should reject an empty transaction hash before polling', async () => {
      await expect(service.monitorTransaction('')).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should reject a non-string transaction hash', async () => {
      await expect(
        service.monitorTransaction(undefined as unknown as string)
      ).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should reject a non-positive timeout', async () => {
      await expect(service.monitorTransaction('tx_hash_123', 0)).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
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
      mockedAxios.get.mockRejectedValue(new Error('Connection failed'));
      mockSorobanServer.getHealth.mockRejectedValue(new Error('Connection failed'));

      const result = await service.healthCheck();

      expect(result.horizon).toBe(false);
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
      mockSorobanServer.prepareTransaction.mockRejectedValue(new Error('prepare failed'));

      await expect(
        service.buildDepositTransaction(VALID_USER_ADDRESS, undefined, '1000000', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build deposit transaction');
    });

    it('should reject a malformed user address before building', async () => {
      await expect(
        service.buildDepositTransaction('not-an-address', undefined, '1000000', VALID_USER_SECRET)
      ).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should reject a malformed secret key before building', async () => {
      await expect(
        service.buildDepositTransaction(VALID_USER_ADDRESS, undefined, '1000000', 'not-a-secret')
      ).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should reject a non-positive deposit amount', async () => {
      await expect(
        service.buildDepositTransaction(VALID_USER_ADDRESS, undefined, '0', VALID_USER_SECRET)
      ).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should reject a non-numeric deposit amount', async () => {
      await expect(
        service.buildDepositTransaction(VALID_USER_ADDRESS, undefined, 'abc', VALID_USER_SECRET)
      ).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should reject a malformed contract id when provided', async () => {
      await expect(
        service.buildDepositTransaction(
          VALID_USER_ADDRESS,
          'not-a-contract',
          '1000000',
          VALID_USER_SECRET
        )
      ).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should accept a valid contract id', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      const result = await service.buildDepositTransaction(
        VALID_USER_ADDRESS,
        VALID_CONTRACT_ID,
        '1000000',
        VALID_USER_SECRET
      );

      expect(result).toBe('prepared_tx_xdr');
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
      mockSorobanServer.prepareTransaction.mockRejectedValue(new Error('prepare failed'));

      await expect(
        service.buildBorrowTransaction(VALID_USER_ADDRESS, undefined, '1000000', VALID_USER_SECRET)
      ).rejects.toThrow('Failed to build borrow transaction');
    });

    it('should reject a malformed user address before building', async () => {
      await expect(
        service.buildBorrowTransaction('not-an-address', undefined, '1000000', VALID_USER_SECRET)
      ).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should reject a malformed secret key before building', async () => {
      await expect(
        service.buildBorrowTransaction(VALID_USER_ADDRESS, undefined, '1000000', 'not-a-secret')
      ).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should reject a non-positive borrow amount', async () => {
      await expect(
        service.buildBorrowTransaction(VALID_USER_ADDRESS, undefined, '0', VALID_USER_SECRET)
      ).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should reject a non-numeric borrow amount', async () => {
      await expect(
        service.buildBorrowTransaction(VALID_USER_ADDRESS, undefined, 'abc', VALID_USER_SECRET)
      ).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should reject a malformed contract id when provided', async () => {
      await expect(
        service.buildBorrowTransaction(
          VALID_USER_ADDRESS,
          'not-a-contract',
          '1000000',
          VALID_USER_SECRET
        )
      ).rejects.toThrow();
      expect(mockedAxios.get).not.toHaveBeenCalled();
    });

    it('should accept a valid contract id', async () => {
      mockedAxios.get.mockResolvedValue({
        data: { id: VALID_USER_ADDRESS, sequence: '123456789' },
      });

      const result = await service.buildBorrowTransaction(
        VALID_USER_ADDRESS,
        VALID_CONTRACT_ID,
        '1000000',
        VALID_USER_SECRET
      );

      expect(result).toBe('prepared_tx_xdr');
    });
  });
});
