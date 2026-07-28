import { StellarService, FetchActivitiesOptions } from './stellar.service';

// Mock stellar-sdk
jest.mock('stellar-sdk', () => ({
  Horizon: {
    Server: jest.fn().mockImplementation(() => ({
      effects: () => ({
        forLedger: jest.fn().mockReturnThis(),
        limit: jest.fn().mockReturnThis(),
        order: jest.fn().mockReturnThis(),
        call: jest.fn().mockResolvedValue({
          records: [
            {
              id: '1',
              type: 'contract_payment',
              ledger: 5000,
              paging_token: '000005000000-0000000002',
              created_at: '2024-01-01T00:00:00Z',
              amount: '100.0000000',
              asset: 'USDC',
              account: 'GACCOUNT',
              source_account: 'GACCOUNT',
              transaction_hash: 'TXHASH1',
            },
            {
              id: '2',
              type: 'contract_offer',
              ledger: 4999,
              paging_token: '000004999000-0000000001',
              created_at: '2024-01-01T00:01:00Z',
              amount: '200.0000000',
              asset: 'XLM',
              account: 'GACCOUNT2',
              source_account: 'GACCOUNT2',
              transaction_hash: 'TXHASH2',
            },
            {
              id: '3',
              type: 'contract_payment',
              ledger: 5000,
              paging_token: '000005000000-0000000001',
              created_at: '2024-01-01T00:02:00Z',
              amount: '50.0000000',
              asset: 'USDC',
              account: 'GACCOUNT3',
              source_account: 'GACCOUNT3',
              transaction_hash: 'TXHASH3',
            },
          ],
        }),
      }),
      ledgers: () => ({
        order: jest.fn().mockReturnThis(),
        limit: jest.fn().mockReturnThis(),
        call: jest.fn().mockResolvedValue({
          records: [{ sequence: 5001 }],
        }),
      }),
    })),
  },
}));

describe('StellarService', () => {
  let service: StellarService;

  beforeEach(() => {
    service = new StellarService();
  });

  describe('fetchActivities', () => {
    it('returns activities ordered by ledger desc, event index desc', async () => {
      const activities = await service.fetchActivities('CONTRACT_ID');
      
      expect(activities).toHaveLength(3);
      expect(activities[0].ledgerSequence).toBe(5000);
      expect(activities[0].eventIndex).toBe(2);
      expect(activities[1].ledgerSequence).toBe(5000);
      expect(activities[1].eventIndex).toBe(1);
      expect(activities[2].ledgerSequence).toBe(4999);
      expect(activities[2].eventIndex).toBe(1);
    });

    it('filters by fromLedger', async () => {
      const activities = await service.fetchActivities('CONTRACT_ID', {
        fromLedger: 5000,
      });
      
      expect(activities).toHaveLength(2);
      expect(activities.every(a => a.ledgerSequence >= 5000)).toBe(true);
    });

    it('filters by fromLedger and fromEventIndex', async () => {
      const activities = await service.fetchActivities('CONTRACT_ID', {
        fromLedger: 5000,
        fromEventIndex: 1,
      });
      
      // Should skip event index 0 and 1 in ledger 5000, return only index 2
      expect(activities).toHaveLength(1);
      expect(activities[0].eventIndex).toBe(2);
    });

    it('limits results', async () => {
      const activities = await service.fetchActivities('CONTRACT_ID', {
        limit: 2,
      });
      
      expect(activities).toHaveLength(2);
    });

    it('returns empty array for future ledger', async () => {
      const activities = await service.fetchActivities('CONTRACT_ID', {
        fromLedger: 9999,
      });
      
      expect(activities).toHaveLength(0);
    });

    it('supports ascending order', async () => {
      const activities = await service.fetchActivities('CONTRACT_ID', {
        order: 'asc',
      });
      
      expect(activities[0].ledgerSequence).toBe(4999);
      expect(activities[2].ledgerSequence).toBe(5000);
    });
  });

  describe('getCurrentLedgerSequence', () => {
    it('returns latest ledger sequence', async () => {
      const seq = await service.getCurrentLedgerSequence();
      expect(seq).toBe(5001);
    });
  });
});