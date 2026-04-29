import request from 'supertest';
import app from '../app';

describe('Vault API Integration Tests', () => {
  describe('POST /vaults', () => {
    const validVaultPayload = {
      amount: 1000,
      endTimestamp: Math.floor(Date.now() / 1000) + 3600, // 1 hour from now
      destination: 'test-destination',
      milestones: [
        { timestamp: Math.floor(Date.now() / 1000) + 1800, description: 'First milestone' },
        { timestamp: Math.floor(Date.now() / 1000) + 2700, description: 'Second milestone', amount: 500 },
      ],
      metadata: { key: 'value' },
      tags: ['tag1', 'tag2'],
    };

    describe('Valid Payloads', () => {
      it('should create vault with minimal valid payload', async () => {
        const minimalPayload = {
          amount: 1000,
          endTimestamp: Math.floor(Date.now() / 1000) + 3600,
          destination: 'test-destination',
          milestones: [],
        };

        const response = await request(app)
          .post('/vaults')
          .send(minimalPayload);

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
        expect(response.body.data).toHaveProperty('id');
        expect(response.body.data).toHaveProperty('createdAt');
        expect(response.body.data.amount).toBe(minimalPayload.amount);
        expect(response.body.data.destination).toBe(minimalPayload.destination);
      });

      it('should create vault with complete valid payload', async () => {
        const response = await request(app)
          .post('/vaults')
          .send(validVaultPayload);

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
        expect(response.body.data).toHaveProperty('id');
        expect(response.body.data).toHaveProperty('createdAt');
        expect(response.body.data.amount).toBe(validVaultPayload.amount);
        expect(response.body.data.destination).toBe(validVaultPayload.destination);
        expect(response.body.data.milestones).toEqual(validVaultPayload.milestones);
        expect(response.body.data.metadata).toEqual(validVaultPayload.metadata);
        expect(response.body.data.tags).toEqual(validVaultPayload.tags);
      });

      it('should create vault without optional fields', async () => {
        const payloadWithoutOptionals = {
          amount: 500,
          endTimestamp: Math.floor(Date.now() / 1000) + 7200,
          destination: 'minimal-destination',
        };

        const response = await request(app)
          .post('/vaults')
          .send(payloadWithoutOptionals);

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
        expect(response.body.data).toHaveProperty('id');
        expect(response.body.data.amount).toBe(payloadWithoutOptionals.amount);
        expect(response.body.data.destination).toBe(payloadWithoutOptionals.destination);
      });
    });

    describe('Amount Validation', () => {
      it('should reject amount below minimum (0)', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, amount: 0 });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject negative amount', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, amount: -100 });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject non-integer amount', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, amount: 1000.5 });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject string amount', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, amount: '1000' });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject missing amount', async () => {
        const { amount, ...payloadWithoutAmount } = validVaultPayload;

        const response = await request(app)
          .post('/vaults')
          .send(payloadWithoutAmount);

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject null amount', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, amount: null });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should accept maximum safe integer amount', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, amount: Number.MAX_SAFE_INTEGER });

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
      });
    });

    describe('End Timestamp Validation', () => {
      it('should reject past timestamp', async () => {
        const pastTimestamp = Math.floor(Date.now() / 1000) - 3600;

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, endTimestamp: pastTimestamp });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject timestamp too soon (less than 1 minute)', async () => {
        const tooSoonTimestamp = Math.floor(Date.now() / 1000) + 30;

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, endTimestamp: tooSoonTimestamp });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject timestamp too far in future', async () => {
        const tooFarTimestamp = Math.floor(Date.now() / 1000) + (365 * 24 * 60 * 60 + 1);

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, endTimestamp: tooFarTimestamp });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject non-numeric timestamp', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, endTimestamp: 'invalid-timestamp' });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject float timestamp', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, endTimestamp: 1234567890.5 });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject missing endTimestamp', async () => {
        const { endTimestamp, ...payloadWithoutTimestamp } = validVaultPayload;

        const response = await request(app)
          .post('/vaults')
          .send(payloadWithoutTimestamp);

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should accept timestamp exactly at minimum boundary', async () => {
        const minTimestamp = Math.floor(Date.now() / 1000) + 60;

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, endTimestamp: minTimestamp });

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
      });
    });

    describe('Destination Validation', () => {
      it('should reject empty destination', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, destination: '' });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject destination with special characters', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, destination: 'invalid@destination' });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject destination with spaces', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, destination: 'invalid destination' });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject destination exceeding maximum length', async () => {
        const longDestination = 'a'.repeat(256);

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, destination: longDestination });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject non-string destination', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, destination: 123 });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject missing destination', async () => {
        const { destination, ...payloadWithoutDestination } = validVaultPayload;

        const response = await request(app)
          .post('/vaults')
          .send(payloadWithoutDestination);

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should accept destination at maximum length', async () => {
        const maxDestination = 'a'.repeat(255);

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, destination: maxDestination });

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
      });
    });

    describe('Milestones Validation', () => {
      it('should reject milestones exceeding maximum array size', async () => {
        const tooManyMilestones = Array.from({ length: 101 }, (_, i) => ({
          timestamp: Math.floor(Date.now() / 1000) + (i + 1) * 3600,
          description: `Milestone ${i + 1}`,
        }));

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, milestones: tooManyMilestones });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject non-array milestones', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, milestones: 'not-an-array' });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject milestone with missing timestamp', async () => {
        const invalidMilestones = [{ description: 'Missing timestamp' }];

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, milestones: invalidMilestones });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject milestone with negative timestamp', async () => {
        const invalidMilestones = [{ timestamp: -1000, description: 'Negative timestamp' }];

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, milestones: invalidMilestones });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject milestones not in chronological order', async () => {
        const invalidMilestones = [
          { timestamp: Math.floor(Date.now() / 1000) + 2700, description: 'Later milestone' },
          { timestamp: Math.floor(Date.now() / 1000) + 1800, description: 'Earlier milestone' },
        ];

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, milestones: invalidMilestones });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should accept empty milestones array', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, milestones: [] });

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
      });

      it('should accept milestones at maximum array size', async () => {
        const maxMilestones = Array.from({ length: 100 }, (_, i) => ({
          timestamp: Math.floor(Date.now() / 1000) + (i + 1) * 3600,
          description: `Milestone ${i + 1}`,
        }));

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, milestones: maxMilestones });

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
      });
    });

    describe('Metadata Validation', () => {
      it('should reject non-object metadata', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, metadata: 'not-an-object' });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject null metadata', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, metadata: null });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject metadata exceeding size limit', async () => {
        const largeMetadata = { data: 'x'.repeat(10241) }; // Exceeds 10KB

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, metadata: largeMetadata });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should accept valid metadata object', async () => {
        const validMetadata = { key: 'value', nested: { prop: 'test' } };

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, metadata: validMetadata });

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
      });

      it('should accept missing metadata', async () => {
        const { metadata, ...payloadWithoutMetadata } = validVaultPayload;

        const response = await request(app)
          .post('/vaults')
          .send(payloadWithoutMetadata);

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
      });
    });

    describe('Tags Validation', () => {
      it('should reject tags exceeding maximum array size', async () => {
        const tooManyTags = Array.from({ length: 21 }, (_, i) => `tag${i + 1}`);

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, tags: tooManyTags });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject non-array tags', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, tags: 'not-an-array' });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject tag with special characters', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, tags: ['invalid@tag'] });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should reject tag exceeding length limit', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, tags: ['a'.repeat(51)] });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should accept empty tags array', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, tags: [] });

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
      });

      it('should accept missing tags', async () => {
        const { tags, ...payloadWithoutTags } = validVaultPayload;

        const response = await request(app)
          .post('/vaults')
          .send(payloadWithoutTags);

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
      });
    });

    describe('Security and Edge Cases', () => {
      it('should handle extremely large payload safely', async () => {
        const largePayload = {
          amount: Number.MAX_SAFE_INTEGER,
          endTimestamp: Math.floor(Date.now() / 1000) + 3600,
          destination: 'a'.repeat(255),
          milestones: Array.from({ length: 100 }, (_, i) => ({
            timestamp: Math.floor(Date.now() / 1000) + (i + 1) * 3600,
            description: 'x'.repeat(500),
            amount: Number.MAX_SAFE_INTEGER,
          })),
          metadata: { data: 'y'.repeat(5000) }, // Within 10KB limit
          tags: Array.from({ length: 20 }, (_, i) => `tag${i}`),
        };

        const response = await request(app)
          .post('/vaults')
          .send(largePayload);

        expect(response.status).toBe(201);
        expect(response.body.success).toBe(true);
      });

      it('should reject maliciously nested objects in metadata', async () => {
        const maliciousMetadata = {
          a: { b: { c: { d: { e: { f: 'g'.repeat(1000) } } } } }
        };

        const response = await request(app)
          .post('/vaults')
          .send({ ...validVaultPayload, metadata: maliciousMetadata });

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should provide consistent error response format', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({ amount: -1 });

        expect(response.status).toBe(400);
        expect(response.body).toHaveProperty('success', false);
        expect(response.body).toHaveProperty('message');
      });

      it('should handle completely empty payload', async () => {
        const response = await request(app)
          .post('/vaults')
          .send({});

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should handle null payload', async () => {
        const response = await request(app)
          .post('/vaults')
          .send(null);

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
      });

      it('should validate all fields simultaneously', async () => {
        const invalidPayload = {
          amount: -1,
          endTimestamp: 'invalid',
          destination: '',
          milestones: 'not-an-array',
          metadata: 'not-an-object',
          tags: 123,
        };

        const response = await request(app)
          .post('/vaults')
          .send(invalidPayload);

        expect(response.status).toBe(400);
        expect(response.body.success).toBe(false);
        expect(response.body).toHaveProperty('message');
      });
    });

    describe('Content-Type and Request Format', () => {
      it('should reject non-JSON content type', async () => {
        const response = await request(app)
          .post('/vaults')
          .set('Content-Type', 'text/plain')
          .send('invalid data');

        expect(response.status).toBe(400);
      });

      it('should handle malformed JSON', async () => {
        const response = await request(app)
          .post('/vaults')
          .set('Content-Type', 'application/json')
          .send('{"invalid": json}');

        expect(response.status).toBe(400);
      });
    });
  });

  describe('PUT /vaults/:id', () => {
    const validUpdatePayload = {
      amount: 2000,
      destination: 'updated-destination',
    };

    it('should accept valid partial update', async () => {
      const response = await request(app)
        .put('/vaults/test-id')
        .send(validUpdatePayload);

      // Should return 404 since vault doesn't exist, but validation should pass
      expect(response.status).toBe(404);
      expect(response.body.success).toBe(false);
      expect(response.body.message).toBe('Vault not found');
    });

    it('should reject invalid amount in update', async () => {
      const response = await request(app)
        .put('/vaults/test-id')
        .send({ amount: -1 });

      expect(response.status).toBe(400);
      expect(response.body.success).toBe(false);
    });

    it('should accept empty update payload', async () => {
      const response = await request(app)
        .put('/vaults/test-id')
        .send({});

      expect(response.status).toBe(404);
      expect(response.body.success).toBe(false);
    });
  });

  describe('GET /vaults', () => {
    it('should return empty vaults list', async () => {
      const response = await request(app)
        .get('/vaults');

      expect(response.status).toBe(200);
      expect(response.body.success).toBe(true);
      expect(response.body.data).toEqual([]);
    });
  });

  describe('GET /vaults/:id', () => {
    it('should return 404 for non-existent vault', async () => {
      const response = await request(app)
        .get('/vaults/non-existent-id');

      expect(response.status).toBe(404);
      expect(response.body.success).toBe(false);
      expect(response.body.message).toBe('Vault not found');
    });
  });

  describe('DELETE /vaults/:id', () => {
    it('should return 404 for non-existent vault', async () => {
      const response = await request(app)
        .delete('/vaults/non-existent-id');

      expect(response.status).toBe(404);
      expect(response.body.success).toBe(false);
      expect(response.body.message).toBe('Vault not found');
    });
  });
});
