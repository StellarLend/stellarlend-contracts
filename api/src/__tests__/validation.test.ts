import request from 'supertest';
import app from '../app';

describe('Validation Middleware', () => {
  describe('Deposit Validation', () => {
    it('should reject empty userAddress', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          amount: '1000000',
          userSecret: 'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
        });

      expect(response.status).toBe(400);
      expect(response.body.success).toBe(false);
    });

    it('should reject zero amount', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
          amount: '0',
          userSecret: 'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
        });

      expect(response.status).toBe(400);
    });

    it('should reject negative amount', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
          amount: '-1000',
          userSecret: 'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
        });

      expect(response.status).toBe(400);
    });

    it('should reject missing userSecret', async () => {
      const response = await request(app)
        .post('/api/lending/deposit')
        .send({
          userAddress: 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX',
          amount: '1000000',
        });

      expect(response.status).toBe(400);
    });
  });

  describe('Borrow Validation', () => {
    it('should validate all required fields', async () => {
      const response = await request(app)
        .post('/api/lending/borrow')
        .send({});

      expect(response.status).toBe(400);
    });
  });

  describe('Repay Validation', () => {
    it('should validate all required fields', async () => {
      const response = await request(app)
        .post('/api/lending/repay')
        .send({});

      expect(response.status).toBe(400);
    });
  });

  describe('Withdraw Validation', () => {
    it('should validate all required fields', async () => {
      const response = await request(app)
        .post('/api/lending/withdraw')
        .send({});

      expect(response.status).toBe(400);
    });
  });
});
