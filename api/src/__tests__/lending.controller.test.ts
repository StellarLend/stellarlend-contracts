import request from 'supertest';
import app from '../app';
import { StellarService } from '../services/stellar.service';

jest.mock('../services/stellar.service');

describe('Lending Controller', () => {
  let mockStellarService: jest.Mocked<StellarService>;

  beforeEach(() => {
    mockStellarService = new StellarService() as jest.Mocked<StellarService>;
    jest.clearAllMocks();
  });

  describe('POST /api/lending/deposit', () => {
    it('should successfully process a deposit', async () => {
      const mockTxXdr = 'mock_tx_xdr';
      const mockTxHash = 'mock_tx_hash';

      mockStellarService.buildDepositTransaction = jest.fn().mockResolvedValue(mockTxXdr);
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
        .post('/api/lending/deposit')
        .send({
          userAddress: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
          amount: '1000000',
          userSecret: 'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
        });

      expect(response.status).toBe(200);
      expect(response.body.success).toBe(true);
      expect(response.body.transactionHash).toBe(mockTxHash);
    });

    it('should return 400 for invalid amount', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
          amount: '0',
          userSecret: 'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
        });

      expect(response.status).toBe(400);
    });

    it('should return 400 for missing required fields', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
        });

      expect(response.status).toBe(400);
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
