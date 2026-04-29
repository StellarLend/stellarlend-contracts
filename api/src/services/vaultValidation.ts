import { body, ValidationChain } from 'express-validator';

// Constants for validation bounds
export const VAULT_CONSTRAINTS = {
  MIN_AMOUNT: 1,
  MAX_AMOUNT: Number.MAX_SAFE_INTEGER,
  MIN_END_TIMESTAMP_OFFSET: 60, // 1 minute from now
  MAX_END_TIMESTAMP_OFFSET: 365 * 24 * 60 * 60, // 1 year from now
  MAX_MILESTONES: 100,
  MAX_DESTINATION_LENGTH: 255,
  MAX_PAYLOAD_SIZE: 1024 * 1024, // 1MB
} as const;

/**
 * Validates the createVault payload with comprehensive boundary checks
 */
export const createVaultSchema: ValidationChain[] = [
  // Amount validation - must be a positive integer within bounds
  body('amount')
    .isInt({ min: VAULT_CONSTRAINTS.MIN_AMOUNT, max: VAULT_CONSTRAINTS.MAX_AMOUNT })
    .withMessage(`Amount must be an integer between ${VAULT_CONSTRAINTS.MIN_AMOUNT} and ${VAULT_CONSTRAINTS.MAX_AMOUNT}`)
    .toInt(),
  
  // End timestamp validation - must be future date within reasonable window
  body('endTimestamp')
    .isInt()
    .withMessage('End timestamp must be an integer')
    .custom((value) => {
      const now = Math.floor(Date.now() / 1000);
      const minTimestamp = now + VAULT_CONSTRAINTS.MIN_END_TIMESTAMP_OFFSET;
      const maxTimestamp = now + VAULT_CONSTRAINTS.MAX_END_TIMESTAMP_OFFSET;
      
      if (value < minTimestamp) {
        throw new Error(`End timestamp must be at least ${VAULT_CONSTRAINTS.MIN_END_TIMESTAMP_OFFSET} seconds in the future`);
      }
      
      if (value > maxTimestamp) {
        throw new Error(`End timestamp must not be more than ${VAULT_CONSTRAINTS.MAX_END_TIMESTAMP_OFFSET} seconds in the future`);
      }
      
      return true;
    }),
  
  // Destination validation - string format and length
  body('destination')
    .isString()
    .withMessage('Destination must be a string')
    .isLength({ min: 1, max: VAULT_CONSTRAINTS.MAX_DESTINATION_LENGTH })
    .withMessage(`Destination must be between 1 and ${VAULT_CONSTRAINTS.MAX_DESTINATION_LENGTH} characters`)
    .matches(/^[a-zA-Z0-9\-_]+$/)
    .withMessage('Destination can only contain alphanumeric characters, hyphens, and underscores'),
  
  // Milestones validation - array with size limits and structure validation
  body('milestones')
    .isArray({ max: VAULT_CONSTRAINTS.MAX_MILESTONES })
    .withMessage(`Milestones must be an array with at most ${VAULT_CONSTRAINTS.MAX_MILESTONES} items`)
    .custom((milestones) => {
      if (!Array.isArray(milestones)) {
        throw new Error('Milestones must be an array');
      }
      
      // Validate each milestone object
      for (let i = 0; i < milestones.length; i++) {
        const milestone = milestones[i];
        
        if (typeof milestone !== 'object' || milestone === null) {
          throw new Error(`Milestone at index ${i} must be an object`);
        }
        
        // Validate milestone timestamp
        if (!('timestamp' in milestone) || typeof milestone.timestamp !== 'number') {
          throw new Error(`Milestone at index ${i} must have a numeric timestamp`);
        }
        
        if (milestone.timestamp <= 0) {
          throw new Error(`Milestone timestamp at index ${i} must be positive`);
        }
        
        // Validate milestone description (optional)
        if ('description' in milestone) {
          if (typeof milestone.description !== 'string') {
            throw new Error(`Milestone description at index ${i} must be a string`);
          }
          
          if (milestone.description.length > 500) {
            throw new Error(`Milestone description at index ${i} must not exceed 500 characters`);
          }
        }
        
        // Validate milestone amount (optional)
        if ('amount' in milestone) {
          if (typeof milestone.amount !== 'number') {
            throw new Error(`Milestone amount at index ${i} must be a number`);
          }
          
          if (milestone.amount < 0) {
            throw new Error(`Milestone amount at index ${i} must be non-negative`);
          }
        }
      }
      
      // Validate milestones are in chronological order
      for (let i = 1; i < milestones.length; i++) {
        if (milestones[i].timestamp <= milestones[i - 1].timestamp) {
          throw new Error(`Milestones must be in chronological order (timestamp at index ${i} must be greater than timestamp at index ${i - 1})`);
        }
      }
      
      return true;
    }),
  
  // Additional optional fields with validation
  body('metadata')
    .optional()
    .isObject()
    .withMessage('Metadata must be an object')
    .custom((metadata) => {
      if (typeof metadata !== 'object' || metadata === null) {
        throw new Error('Metadata must be a valid object');
      }
      
      // Check metadata size to prevent overly large payloads
      const metadataSize = JSON.stringify(metadata).length;
      if (metadataSize > 10240) { // 10KB limit for metadata
        throw new Error('Metadata size must not exceed 10KB');
      }
      
      return true;
    }),
  
  body('tags')
    .optional()
    .isArray({ max: 20 })
    .withMessage('Tags must be an array with at most 20 items')
    .custom((tags) => {
      if (!Array.isArray(tags)) {
        throw new Error('Tags must be an array');
      }
      
      for (let i = 0; i < tags.length; i++) {
        const tag = tags[i];
        if (typeof tag !== 'string') {
          throw new Error(`Tag at index ${i} must be a string`);
        }
        
        if (tag.length > 50) {
          throw new Error(`Tag at index ${i} must not exceed 50 characters`);
        }
        
        if (!/^[a-zA-Z0-9\-_]+$/.test(tag)) {
          throw new Error(`Tag at index ${i} can only contain alphanumeric characters, hyphens, and underscores`);
        }
      }
      
      return true;
    }),
];

/**
 * Validates vault update payload (subset of create validation)
 */
export const updateVaultSchema: ValidationChain[] = [
  body('amount')
    .optional()
    .isInt({ min: VAULT_CONSTRAINTS.MIN_AMOUNT, max: VAULT_CONSTRAINTS.MAX_AMOUNT })
    .withMessage(`Amount must be an integer between ${VAULT_CONSTRAINTS.MIN_AMOUNT} and ${VAULT_CONSTRAINTS.MAX_AMOUNT}`)
    .toInt(),
  
  body('endTimestamp')
    .optional()
    .isInt()
    .withMessage('End timestamp must be an integer')
    .custom((value) => {
      const now = Math.floor(Date.now() / 1000);
      const minTimestamp = now + VAULT_CONSTRAINTS.MIN_END_TIMESTAMP_OFFSET;
      const maxTimestamp = now + VAULT_CONSTRAINTS.MAX_END_TIMESTAMP_OFFSET;
      
      if (value < minTimestamp) {
        throw new Error(`End timestamp must be at least ${VAULT_CONSTRAINTS.MIN_END_TIMESTAMP_OFFSET} seconds in the future`);
      }
      
      if (value > maxTimestamp) {
        throw new Error(`End timestamp must not be more than ${VAULT_CONSTRAINTS.MAX_END_TIMESTAMP_OFFSET} seconds in the future`);
      }
      
      return true;
    }),
  
  body('destination')
    .optional()
    .isString()
    .withMessage('Destination must be a string')
    .isLength({ min: 1, max: VAULT_CONSTRAINTS.MAX_DESTINATION_LENGTH })
    .withMessage(`Destination must be between 1 and ${VAULT_CONSTRAINTS.MAX_DESTINATION_LENGTH} characters`)
    .matches(/^[a-zA-Z0-9\-_]+$/)
    .withMessage('Destination can only contain alphanumeric characters, hyphens, and underscores'),
  
  body('milestones')
    .optional()
    .isArray({ max: VAULT_CONSTRAINTS.MAX_MILESTONES })
    .withMessage(`Milestones must be an array with at most ${VAULT_CONSTRAINTS.MAX_MILESTONES} items`),
  
  body('metadata')
    .optional()
    .isObject()
    .withMessage('Metadata must be an object'),
  
  body('tags')
    .optional()
    .isArray({ max: 20 })
    .withMessage('Tags must be an array with at most 20 items'),
];
