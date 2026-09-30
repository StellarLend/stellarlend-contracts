import {
  encodeCursor,
  decodeCursor,
  isValidCursor,
  getNextCursor,
  compareCursors,
  CursorError,
} from './cursor';

describe('cursor utilities', () => {
  describe('encodeCursor', () => {
    it('encodes valid ledger sequence and event index', () => {
      const cursor = encodeCursor({ ledgerSequence: 1000, eventIndex: 5 });
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

    it('returns false for invalid cursor', () => {
      expect(isValidCursor('garbage')).toBe(false);
    });

    it('returns false for empty string', () => {
      expect(isValidCursor('')).toBe(false);
    });

    it('returns false for non-string', () => {
      expect(isValidCursor(null)).toBe(false);
      expect(isValidCursor(123)).toBe(false);
    });
  });

  describe('getNextCursor', () => {
    it('returns cursor for last item', () => {
      const items = [
        { ledgerSequence: 100, eventIndex: 0 },
        { ledgerSequence: 100, eventIndex: 1 },
        { ledgerSequence: 101, eventIndex: 0 },
      ];
      expect(decodeCursor(getNextCursor(items)!)).toEqual({ ledgerSequence: 101, eventIndex: 0 });
    });

    it('returns undefined for empty array', () => {
      expect(getNextCursor([])).toBeUndefined();
    });
  });

  describe('compareCursors', () => {
    it('returns negative when a < b (ledger)', () => {
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
