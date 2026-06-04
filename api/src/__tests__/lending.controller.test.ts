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
    jest.clearAllMocks();
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
});