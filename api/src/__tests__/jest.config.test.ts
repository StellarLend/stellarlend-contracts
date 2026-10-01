/**
 * Invariants for `api/jest.config.js`.
 *
 * The Jest config is executable configuration: a careless edit (dropping
 * `ts-jest`, widening `roots`/`testMatch` beyond `src`, or lowering the
 * coverage gate) silently weakens the whole API test suite. These tests pin the
 * load-bearing settings so such a change fails loudly.
 */
/* eslint-disable @typescript-eslint/no-var-requires */
const config = require('../../jest.config.js');

describe('jest.config.js invariants', () => {
  it('runs TypeScript through ts-jest on a Node environment', () => {
    expect(config.preset).toBe('ts-jest');
    expect(config.testEnvironment).toBe('node');
    expect(config.transform).toEqual({ '^.+\\.ts$': 'ts-jest' });
  });

  it('only discovers tests under <rootDir>/src', () => {
    expect(config.roots).toEqual(['<rootDir>/src']);
    expect(config.testMatch).toEqual([
      '**/__tests__/**/*.ts',
      '**/?(*.)+(spec|test).ts',
    ]);
  });

  it('collects coverage from src but excludes test files and the entrypoint', () => {
    expect(config.collectCoverageFrom).toEqual(
      expect.arrayContaining([
        'src/**/*.ts',
        '!src/**/*.test.ts',
        '!src/**/*.spec.ts',
        '!src/index.ts',
      ])
    );
  });

  it('keeps the 95% global coverage gate for every metric', () => {
    expect(config.coverageThreshold).toEqual({
      global: {
        branches: 95,
        functions: 95,
        lines: 95,
        statements: 95,
      },
    });
  });

  it('writes coverage to the ./coverage directory', () => {
    expect(config.coverageDirectory).toBe('coverage');
  });

  it('emits verbose output (useful for CI debugging)', () => {
    expect(config.verbose).toBe(true);
  });
});
