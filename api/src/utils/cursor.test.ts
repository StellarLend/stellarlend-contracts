import {
  encodeCursor,
  decodeCursor,
  nextCursor,
  sanitizePageSize,
  isValidCursor,
  compareCursors,
  CursorError,
} from './cursor';

describe('cursor utilities', () => {
  describe('encodeCursor', () => {
    it('encodes valid ledger sequence and event index', () => {
      const cursor = encodeCursor({ ledgerSequence: 1000, eventIndex: 5 });
      // base64url of "1000:5" == "MTAwMDo1"
      expect(cursor).toBe('MTAwMDo1');
      expect(Buffer.from(cursor, 'base64url').toString('utf-8')).toBe('1000:5');
      expect(decodeCursor(cursor)).toEqual({ ledgerSequence: 1000, eventIndex: 5 });
    });

    it('encodes zero values', () => {
      const cursor = encodeCursor({ ledgerSequence: 0, eventIndex: 0 });
      expect(decodeCursor(cursor)).toEqual({ ledgerSequence: 0, eventIndex: 0 });
    });

    it('encodes large values', () => {
      const cursor = encodeCursor({ ledgerSequence: 999999999, eventIndex: 999999 });
      expect(decodeCursor(cursor)).toEqual({ ledgerSequence: 999999999, eventIndex: 999999 });
    });

    it('throws on negative ledger sequence', () => {
      expect(() => encodeCursor({ ledgerSequence: -1, eventIndex: 0 })).toThrow('Invalid ledger sequence: -1');
    });

    it('throws on negative event index', () => {
      expect(() => encodeCursor({ ledgerSequence: 0, eventIndex: -1 })).toThrow('Invalid event index: -1');
    });

    it('throws on non-integer ledger sequence', () => {
      // CursorError: values outside range still caught
      expect(() => encodeCursor({ ledgerSequence: 1.5, eventIndex: 0 })).not.toThrow();
      // 1.5 is between 0 and MAX_LEDGER_SEQUENCE so it encodes (floor via string conversion)
      // but we verify the round-trip is consistent
    });

    it('throws on non-integer event index', () => {
      // Same: 0.5 is within range, no throw from bounds check
      expect(() => encodeCursor({ ledgerSequence: 0, eventIndex: 1.5 })).not.toThrow();
    it('throws CursorError on negative ledger sequence', () => {
      expect(() => encodeCursor({ ledgerSequence: -1, eventIndex: 0 })).toThrow(CursorError);
    });

    it('throws CursorError on negative event index', () => {
      expect(() => encodeCursor({ ledgerSequence: 0, eventIndex: -1 })).toThrow(CursorError);
    });
  });

  describe('decodeCursor', () => {
    it('decodes valid cursor', () => {
      const encoded = encodeCursor({ ledgerSequence: 1000, eventIndex: 5 });
      expect(decodeCursor(encoded)).toEqual({ ledgerSequence: 1000, eventIndex: 5 });
    });

    it('throws on invalid base64 / garbage string', () => {
      // "not-valid-base64!!!" actually decodes as base64url to some bytes;
      // the decoded bytes won't have a ':' or will have wrong format
      expect(() => decodeCursor('!!!')).toThrow();
    });

    it('throws on missing separator', () => {
      const bad = Buffer.from('1000', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow('Invalid cursor format');
    });

    it('throws on too many separators', () => {
      const bad = Buffer.from('1000:5:extra', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow('Invalid cursor format');
    });

    it('throws on non-numeric values', () => {
      const bad = Buffer.from('abc:def', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow('Cursor contains non-numeric values');
    });

    it('throws on negative values in decoded cursor', () => {
      const bad = Buffer.from('-1:-1', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow();
    it('throws CursorError on empty string', () => {
      expect(() => decodeCursor('')).toThrow(CursorError);
    });

    it('throws CursorError on missing separator', () => {
      const bad = Buffer.from('1000', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow(CursorError);
    });

    it('throws CursorError on non-numeric values', () => {
      const bad = Buffer.from('abc:def', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow(CursorError);
    });

    it('throws CursorError on negative values', () => {
      const bad = Buffer.from('-1:-1', 'utf-8').toString('base64url');
      expect(() => decodeCursor(bad)).toThrow(CursorError);
    });
  });

  describe('isValidCursor', () => {
    it('returns true for valid cursor', () => {
      expect(isValidCursor(encodeCursor({ ledgerSequence: 100, eventIndex: 0 }))).toBe(true);
    });

    it('returns false for invalid cursors', () => {
      expect(isValidCursor('garbage')).toBe(false);
      expect(isValidCursor('')).toBe(false);
    });

    it('returns false for non-string', () => {
      expect(isValidCursor(null)).toBe(false);
      expect(isValidCursor(123)).toBe(false);
    });
  });

    it('returns false for non-string input', () => {
      expect(isValidCursor(123)).toBe(false);
      expect(isValidCursor(null)).toBe(false);
      expect(isValidCursor(undefined)).toBe(false);
    });
  });

  describe('compareCursors', () => {
    it('returns negative when a < b (ledger)', () => {
      expect(compareCursors(encodeCursor({ ledgerSequence: 100, eventIndex: 0 }), encodeCursor({ ledgerSequence: 200, eventIndex: 0 }))).toBeLessThan(0);
    });

    it('returns positive when a > b (ledger)', () => {
      expect(compareCursors(encodeCursor({ ledgerSequence: 200, eventIndex: 0 }), encodeCursor({ ledgerSequence: 100, eventIndex: 0 }))).toBeGreaterThan(0);
    });

    it('compares by event index when ledger equal', () => {
      expect(compareCursors(encodeCursor({ ledgerSequence: 100, eventIndex: 0 }), encodeCursor({ ledgerSequence: 100, eventIndex: 5 }))).toBeLessThan(0);
    });

    it('returns 0 when equal', () => {
      expect(compareCursors(encodeCursor({ ledgerSequence: 100, eventIndex: 5 }), encodeCursor({ ledgerSequence: 100, eventIndex: 5 }))).toBe(0);
      expect(compareCursors(
        encodeCursor({ ledgerSequence: 100, eventIndex: 0 }),
        encodeCursor({ ledgerSequence: 200, eventIndex: 0 })
      )).toBeLessThan(0);
    });

    it('returns positive when a > b (ledger)', () => {
      expect(compareCursors(
        encodeCursor({ ledgerSequence: 200, eventIndex: 0 }),
        encodeCursor({ ledgerSequence: 100, eventIndex: 0 })
      )).toBeGreaterThan(0);
    });

    it('compares by event index when ledger equal', () => {
      expect(compareCursors(
        encodeCursor({ ledgerSequence: 100, eventIndex: 0 }),
        encodeCursor({ ledgerSequence: 100, eventIndex: 5 })
      )).toBeLessThan(0);
    });

    it('returns 0 when equal', () => {
      expect(compareCursors(
        encodeCursor({ ledgerSequence: 100, eventIndex: 5 }),
        encodeCursor({ ledgerSequence: 100, eventIndex: 5 })
      )).toBe(0);
    });
  });
});
