import {
  encodeCursor,
  decodeCursor,
  isValidCursor,
  getNextCursor,
  compareCursors,
} from './cursor';

describe('cursor utilities', () => {
  describe('encodeCursor', () => {
    it('encodes valid ledger sequence and event index', () => {
      const cursor = encodeCursor(1000, 5);
      expect(cursor).toBe('MTAwMDo1');
      expect(Buffer.from(cursor, 'base64').toString('utf-8')).toBe('1000:5');
    });

    it('encodes zero values', () => {
      const cursor = encodeCursor(0, 0);
      expect(decodeCursor(cursor)).toEqual({ ledgerSequence: 0, eventIndex: 0 });
    });

    it('encodes large values', () => {
      const cursor = encodeCursor(999999999, 999999);
      expect(decodeCursor(cursor)).toEqual({ ledgerSequence: 999999999, eventIndex: 999999 });
    });

    it('throws on negative ledger sequence', () => {
      expect(() => encodeCursor(-1, 0)).toThrow('Invalid ledger sequence: -1');
    });

    it('throws on negative event index', () => {
      expect(() => encodeCursor(0, -1)).toThrow('Invalid event index: -1');
    });

    it('throws on non-integer ledger sequence', () => {
      expect(() => encodeCursor(1.5, 0)).toThrow('Invalid ledger sequence: 1.5');
    });

    it('throws on non-integer event index', () => {
      expect(() => encodeCursor(0, 1.5)).toThrow('Invalid event index: 1.5');
    });
  });

  describe('decodeCursor', () => {
    it('decodes valid cursor', () => {
      const encoded = encodeCursor(1000, 5);
      expect(decodeCursor(encoded)).toEqual({ ledgerSequence: 1000, eventIndex: 5 });
    });

    it('throws on invalid base64', () => {
      expect(() => decodeCursor('not-valid-base64!!!')).toThrow('Cursor decode failed');
    });

    it('throws on missing separator', () => {
      const bad = Buffer.from('1000', 'utf-8').toString('base64');
      expect(() => decodeCursor(bad)).toThrow('Invalid cursor format');
    });

    it('throws on too many separators', () => {
      const bad = Buffer.from('1000:5:extra', 'utf-8').toString('base64');
      expect(() => decodeCursor(bad)).toThrow('Invalid cursor format');
    });

    it('throws on non-numeric values', () => {
      const bad = Buffer.from('abc:def', 'utf-8').toString('base64');
      expect(() => decodeCursor(bad)).toThrow('Invalid cursor: ledger sequence and event index must be integers');
    });

    it('throws on negative values in decoded cursor', () => {
      const bad = Buffer.from('-1:-1', 'utf-8').toString('base64');
      expect(() => decodeCursor(bad)).toThrow('Invalid cursor: values must be non-negative');
    });
  });

  describe('isValidCursor', () => {
    it('returns true for valid cursor', () => {
      expect(isValidCursor(encodeCursor(100, 0))).toBe(true);
    });

    it('returns false for invalid cursor', () => {
      expect(isValidCursor('garbage')).toBe(false);
    });

    it('returns false for empty string', () => {
      expect(isValidCursor('')).toBe(false);
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
      expect(compareCursors(encodeCursor(100, 0), encodeCursor(200, 0))).toBeLessThan(0);
    });

    it('returns positive when a > b (ledger)', () => {
      expect(compareCursors(encodeCursor(200, 0), encodeCursor(100, 0))).toBeGreaterThan(0);
    });

    it('compares by event index when ledger equal', () => {
      expect(compareCursors(encodeCursor(100, 0), encodeCursor(100, 5))).toBeLessThan(0);
    });

    it('returns 0 when equal', () => {
      expect(compareCursors(encodeCursor(100, 5), encodeCursor(100, 5))).toBe(0);
    });
  });
});