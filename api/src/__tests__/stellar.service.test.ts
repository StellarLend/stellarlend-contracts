import { StellarService } from '../services/stellar.service';
import axios from 'axios';
import { rpc } from '@stellar/stellar-sdk';

jest.mock('axios');
jest.mock('@stellar/stellar-sdk');
jest.mock('@stellar/stellar-sdk/rpc');

const mockedAxios = axios as jest.Mocked<typeof axios>;

describe('StellarService', () => {
  let service: StellarService;

  beforeEach(() => {
    service = new StellarService();
    jest.clearAllMocks();
  });

  describe('getAccount', () => {
    it('should fetch account information', async () => {
      const mockAccountData = {
        id: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
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
  });

  describe('healthCheck', () => {
    it('should return healthy status for all services', async () => {
      mockedAxios.get.mockResolvedValue({ data: {} });
      
      const mockSorobanServer = {
        getHealth: jest.fn().mockResolvedValue({}),
      };
      (rpc.Server as jest.Mock).mockImplementation(() => mockSorobanServer);

      const result = await service.healthCheck();

      expect(result.horizon).toBe(true);
      expect(result.sorobanRpc).toBe(true);
    });

    it('should return unhealthy status when services fail', async () => {
      mockedAxios.get.mockRejectedValue(new Error('Connection failed'));
      
      const mockSorobanServer = {
        getHealth: jest.fn().mockRejectedValue(new Error('Connection failed')),
      };
      (rpc.Server as jest.Mock).mockImplementation(() => mockSorobanServer);

      const result = await service.healthCheck();

      expect(result.horizon).toBe(false);
      expect(result.sorobanRpc).toBe(false);
    });
  });

  describe('buildDepositTransaction', () => {
    it('should build deposit transaction', async () => {
      const mockAccountData = {
        id: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
        sequence: '123456789',
      };

      mockedAxios.get.mockResolvedValue({ data: mockAccountData });

      const mockSorobanServer = {
        prepareTransaction: jest.fn().mockResolvedValue({
          sign: jest.fn(),
          toXDR: jest.fn().mockReturnValue('prepared_tx_xdr'),
        }),
      };
      (rpc.Server as jest.Mock).mockImplementation(() => mockSorobanServer);

      const result = await service.buildDepositTransaction(
        mockAccountData.id,
        undefined,
        '1000000',
        'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX'
      );

      expect(result).toBe('prepared_tx_xdr');
    });
  });
});
