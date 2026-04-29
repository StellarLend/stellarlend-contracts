import { Request } from 'express';
import { body, validationResult } from 'express-validator';
import { createVaultSchema, updateVaultSchema, VAULT_CONSTRAINTS } from './vaultValidation';

describe('Vault Validation', () => {
  let mockRequest: Partial<Request>;

  beforeEach(() => {
    mockRequest = {
      body: {},
    };
  });

  describe('createVaultSchema', () => {
    const validPayload = {
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

    describe('Amount Validation', () => {
      it('should accept valid minimum amount', async () => {
        mockRequest.body = { ...validPayload, amount: VAULT_CONSTRAINTS.MIN_AMOUNT };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should accept valid maximum amount', async () => {
        mockRequest.body = { ...validPayload, amount: VAULT_CONSTRAINTS.MAX_AMOUNT };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should reject amount below minimum', async () => {
        mockRequest.body = { ...validPayload, amount: VAULT_CONSTRAINTS.MIN_AMOUNT - 1 };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain(`between ${VAULT_CONSTRAINTS.MIN_AMOUNT} and`);
      });

      it('should reject amount above maximum', async () => {
        mockRequest.body = { ...validPayload, amount: VAULT_CONSTRAINTS.MAX_AMOUNT + 1 };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain(`between ${VAULT_CONSTRAINTS.MIN_AMOUNT} and`);
      });

      it('should reject non-integer amount', async () => {
        mockRequest.body = { ...validPayload, amount: 1000.5 };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be an integer');
      });

      it('should reject non-numeric string amount', async () => {
        mockRequest.body = { ...validPayload, amount: 'invalid-amount' };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be an integer');
      });

      it('should reject negative amount', async () => {
        mockRequest.body = { ...validPayload, amount: -1000 };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be an integer');
      });

      it('should reject zero amount', async () => {
        mockRequest.body = { ...validPayload, amount: 0 };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be an integer');
      });
    });

    describe('End Timestamp Validation', () => {
      it('should accept valid future timestamp', async () => {
        const futureTimestamp = Math.floor(Date.now() / 1000) + 3600;
        mockRequest.body = { ...validPayload, endTimestamp: futureTimestamp };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should reject past timestamp', async () => {
        const pastTimestamp = Math.floor(Date.now() / 1000) - 3600;
        mockRequest.body = { ...validPayload, endTimestamp: pastTimestamp };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('at least');
      });

      it('should reject timestamp too soon', async () => {
        const tooSoonTimestamp = Math.floor(Date.now() / 1000) + 30; // 30 seconds from now
        mockRequest.body = { ...validPayload, endTimestamp: tooSoonTimestamp };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('at least');
      });

      it('should reject timestamp too far in future', async () => {
        const tooFarTimestamp = Math.floor(Date.now() / 1000) + (VAULT_CONSTRAINTS.MAX_END_TIMESTAMP_OFFSET + 1);
        mockRequest.body = { ...validPayload, endTimestamp: tooFarTimestamp };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must not be more than');
      });

      it('should reject non-numeric timestamp', async () => {
        mockRequest.body = { ...validPayload, endTimestamp: 'invalid-timestamp' };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be an integer');
      });

      it('should reject float timestamp', async () => {
        mockRequest.body = { ...validPayload, endTimestamp: 1234567890.5 };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be an integer');
      });
    });

    describe('Destination Validation', () => {
      it('should accept valid destination', async () => {
        mockRequest.body = { ...validPayload, destination: 'valid-destination_123' };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should accept destination at minimum length', async () => {
        mockRequest.body = { ...validPayload, destination: 'a' };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should accept destination at maximum length', async () => {
        mockRequest.body = { ...validPayload, destination: 'a'.repeat(VAULT_CONSTRAINTS.MAX_DESTINATION_LENGTH) };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should reject empty destination', async () => {
        mockRequest.body = { ...validPayload, destination: '' };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('between 1 and');
      });

      it('should reject destination exceeding maximum length', async () => {
        mockRequest.body = { ...validPayload, destination: 'a'.repeat(VAULT_CONSTRAINTS.MAX_DESTINATION_LENGTH + 1) };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('between 1 and');
      });

      it('should reject destination with special characters', async () => {
        mockRequest.body = { ...validPayload, destination: 'invalid@destination' };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('can only contain');
      });

      it('should reject destination with spaces', async () => {
        mockRequest.body = { ...validPayload, destination: 'invalid destination' };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('can only contain');
      });

      it('should reject non-string destination', async () => {
        mockRequest.body = { ...validPayload, destination: 123 };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be a string');
      });
    });

    describe('Milestones Validation', () => {
      it('should accept valid milestones array', async () => {
        const validMilestones = [
          { timestamp: Math.floor(Date.now() / 1000) + 1800, description: 'First milestone' },
          { timestamp: Math.floor(Date.now() / 1000) + 2700, description: 'Second milestone', amount: 500 },
        ];
        mockRequest.body = { ...validPayload, milestones: validMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should accept empty milestones array', async () => {
        mockRequest.body = { ...validPayload, milestones: [] };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should accept milestones at maximum array size', async () => {
        const maxMilestones = Array.from({ length: VAULT_CONSTRAINTS.MAX_MILESTONES }, (_, i) => ({
          timestamp: Math.floor(Date.now() / 1000) + (i + 1) * 3600,
          description: `Milestone ${i + 1}`,
        }));
        mockRequest.body = { ...validPayload, milestones: maxMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should reject milestones exceeding maximum array size', async () => {
        const tooManyMilestones = Array.from({ length: VAULT_CONSTRAINTS.MAX_MILESTONES + 1 }, (_, i) => ({
          timestamp: Math.floor(Date.now() / 1000) + (i + 1) * 3600,
          description: `Milestone ${i + 1}`,
        }));
        mockRequest.body = { ...validPayload, milestones: tooManyMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('at most');
      });

      it('should reject non-array milestones', async () => {
        mockRequest.body = { ...validPayload, milestones: 'not-an-array' };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be an array');
      });

      it('should reject milestone with missing timestamp', async () => {
        const invalidMilestones = [{ description: 'Missing timestamp' }];
        mockRequest.body = { ...validPayload, milestones: invalidMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must have a numeric timestamp');
      });

      it('should reject milestone with non-numeric timestamp', async () => {
        const invalidMilestones = [{ timestamp: 'invalid', description: 'Invalid timestamp' }];
        mockRequest.body = { ...validPayload, milestones: invalidMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must have a numeric timestamp');
      });

      it('should reject milestone with negative timestamp', async () => {
        const invalidMilestones = [{ timestamp: -1000, description: 'Negative timestamp' }];
        mockRequest.body = { ...validPayload, milestones: invalidMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be positive');
      });

      it('should reject milestone with zero timestamp', async () => {
        const invalidMilestones = [{ timestamp: 0, description: 'Zero timestamp' }];
        mockRequest.body = { ...validPayload, milestones: invalidMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be positive');
      });

      it('should reject milestone with non-object type', async () => {
        const invalidMilestones = ['not-an-object'];
        mockRequest.body = { ...validPayload, milestones: invalidMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be an object');
      });

      it('should reject milestone with null value', async () => {
        const invalidMilestones = [null];
        mockRequest.body = { ...validPayload, milestones: invalidMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be an object');
      });

      it('should reject milestone with description too long', async () => {
        const invalidMilestones = [{
          timestamp: Math.floor(Date.now() / 1000) + 1800,
          description: 'a'.repeat(501), // Exceeds 500 character limit
        }];
        mockRequest.body = { ...validPayload, milestones: invalidMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must not exceed 500 characters');
      });

      it('should reject milestone with non-string description', async () => {
        const invalidMilestones = [{
          timestamp: Math.floor(Date.now() / 1000) + 1800,
          description: 123,
        }];
        mockRequest.body = { ...validPayload, milestones: invalidMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be a string');
      });

      it('should reject milestone with negative amount', async () => {
        const invalidMilestones = [{
          timestamp: Math.floor(Date.now() / 1000) + 1800,
          amount: -100,
        }];
        mockRequest.body = { ...validPayload, milestones: invalidMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be non-negative');
      });

      it('should reject milestone with non-numeric amount', async () => {
        const invalidMilestones = [{
          timestamp: Math.floor(Date.now() / 1000) + 1800,
          amount: 'invalid',
        }];
        mockRequest.body = { ...validPayload, milestones: invalidMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be a number');
      });

      it('should reject milestones not in chronological order', async () => {
        const invalidMilestones = [
          { timestamp: Math.floor(Date.now() / 1000) + 2700, description: 'Later milestone' },
          { timestamp: Math.floor(Date.now() / 1000) + 1800, description: 'Earlier milestone' },
        ];
        mockRequest.body = { ...validPayload, milestones: invalidMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('chronological order');
      });

      it('should reject milestones with duplicate timestamps', async () => {
        const duplicateTimestamp = Math.floor(Date.now() / 1000) + 1800;
        const invalidMilestones = [
          { timestamp: duplicateTimestamp, description: 'First milestone' },
          { timestamp: duplicateTimestamp, description: 'Second milestone' },
        ];
        mockRequest.body = { ...validPayload, milestones: invalidMilestones };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('chronological order');
      });
    });

    describe('Metadata Validation', () => {
      it('should accept valid metadata object', async () => {
        mockRequest.body = { ...validPayload, metadata: { key: 'value', nested: { prop: 'test' } } };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should accept empty metadata object', async () => {
        mockRequest.body = { ...validPayload, metadata: {} };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should accept missing metadata', async () => {
        const { metadata, ...payloadWithoutMetadata } = validPayload;
        mockRequest.body = payloadWithoutMetadata;
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should reject non-object metadata', async () => {
        mockRequest.body = { ...validPayload, metadata: 'not-an-object' };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be an object');
      });

      it('should reject null metadata', async () => {
        mockRequest.body = { ...validPayload, metadata: null };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be an object');
      });

      it('should reject metadata exceeding size limit', async () => {
        const largeMetadata = { data: 'x'.repeat(10241) }; // Exceeds 10KB
        mockRequest.body = { ...validPayload, metadata: largeMetadata };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must not exceed 10KB');
      });
    });

    describe('Tags Validation', () => {
      it('should accept valid tags array', async () => {
        mockRequest.body = { ...validPayload, tags: ['tag1', 'tag2', 'tag-3'] };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should accept empty tags array', async () => {
        mockRequest.body = { ...validPayload, tags: [] };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should accept tags at maximum array size', async () => {
        const maxTags = Array.from({ length: 20 }, (_, i) => `tag${i + 1}`);
        mockRequest.body = { ...validPayload, tags: maxTags };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should reject tags exceeding maximum array size', async () => {
        const tooManyTags = Array.from({ length: 21 }, (_, i) => `tag${i + 1}`);
        mockRequest.body = { ...validPayload, tags: tooManyTags };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('at most 20 items');
      });

      it('should reject non-array tags', async () => {
        mockRequest.body = { ...validPayload, tags: 'not-an-array' };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be an array');
      });

      it('should reject tag with non-string type', async () => {
        mockRequest.body = { ...validPayload, tags: ['valid-tag', 123] };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must be a string');
      });

      it('should reject tag exceeding length limit', async () => {
        mockRequest.body = { ...validPayload, tags: ['a'.repeat(51)] }; // Exceeds 50 characters
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must not exceed 50 characters');
      });

      it('should reject tag with special characters', async () => {
        mockRequest.body = { ...validPayload, tags: ['invalid@tag'] };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('can only contain');
      });
    });

    describe('Security and Edge Cases', () => {
      it('should handle extremely large numbers safely', async () => {
        mockRequest.body = { ...validPayload, amount: Number.MAX_SAFE_INTEGER };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(true);
      });

      it('should reject payload with maliciously nested objects', async () => {
        // Create metadata that exceeds 10KB when serialized
        const largeObject: any = {};
        let current = largeObject;
        for (let i = 0; i < 100; i++) {
          current.nested = { data: 'x'.repeat(100) };
          current = current.nested;
        }
        
        mockRequest.body = { ...validPayload, metadata: largeObject };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('must not exceed 10KB');
      });

      it('should handle Unicode characters in destination', async () => {
        mockRequest.body = { ...validPayload, destination: 'test-ñame' };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].msg).toContain('can only contain');
      });

      it('should provide consistent error field paths', async () => {
        mockRequest.body = { ...validPayload, amount: -1 };
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array()[0].path).toBe('amount');
        expect(errors.array()[0].msg).toBeDefined();
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
        mockRequest.body = invalidPayload;
        
        const validations = createVaultSchema.map(validation => validation.run(mockRequest));
        await Promise.all(validations);
        
        const errors = validationResult(mockRequest);
        expect(errors.isEmpty()).toBe(false);
        expect(errors.array().length).toBeGreaterThan(1);
      });
    });
  });

  describe('updateVaultSchema', () => {
    const validUpdatePayload = {
      amount: 2000,
      destination: 'updated-destination',
    };

    it('should accept valid partial update', async () => {
      mockRequest.body = validUpdatePayload;
      
      const validations = updateVaultSchema.map(validation => validation.run(mockRequest));
      await Promise.all(validations);
      
      const errors = validationResult(mockRequest);
      expect(errors.isEmpty()).toBe(true);
    });

    it('should accept empty update payload', async () => {
      mockRequest.body = {};
      
      const validations = updateVaultSchema.map(validation => validation.run(mockRequest));
      await Promise.all(validations);
      
      const errors = validationResult(mockRequest);
      expect(errors.isEmpty()).toBe(true);
    });

    it('should apply same validation rules as create for provided fields', async () => {
      mockRequest.body = { amount: -1 };
      
      const validations = updateVaultSchema.map(validation => validation.run(mockRequest));
      await Promise.all(validations);
      
      const errors = validationResult(mockRequest);
      expect(errors.isEmpty()).toBe(false);
      expect(errors.array()[0].msg).toContain('must be an integer');
    });
  });
});
