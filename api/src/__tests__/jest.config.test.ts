/**
 * Authorization and Validation Regression Coverage for api/jest.config.js
 *
 * Validates the schema, deterministic behavior, boundary rules,
 * pattern matching invariants, and coverage enforcement of Jest configuration.
 */

import path from 'path';

describe('Jest Configuration (api/jest.config.js)', () => {
  const configPath = path.resolve(__dirname, '../../jest.config.js');
  let jestConfig: any;

  beforeEach(() => {
    // Isolate module cache to test deterministic loading
    jest.resetModules();
    jestConfig = require(configPath);
  });

  describe('Structure and Schema Validation', () => {
    it('should export a valid configuration object', () => {
      expect(jestConfig).toBeDefined();
      expect(typeof jestConfig).toBe('object');
      expect(jestConfig).not.toBeNull();
    });

    it('should have required top-level configuration properties', () => {
      const requiredProperties = [
        'preset',
        'testEnvironment',
        'roots',
        'testMatch',
        'transform',
        'collectCoverageFrom',
        'coverageThreshold',
        'coverageDirectory',
        'verbose',
      ];

      for (const prop of requiredProperties) {
        expect(jestConfig).toHaveProperty(prop);
      }
    });

    it('should configure the TypeScript preset and Node environment', () => {
      expect(jestConfig.preset).toBe('ts-jest');
      expect(jestConfig.testEnvironment).toBe('node');
    });

    it('should enable verbose test reporting', () => {
      expect(jestConfig.verbose).toBe(true);
    });

    it('should configure the expected coverage directory', () => {
      expect(jestConfig.coverageDirectory).toBe('coverage');
    });
  });

  describe('Roots and Path Resolution Invariants', () => {
    it('should restrict roots exclusively to <rootDir>/src', () => {
      expect(Array.isArray(jestConfig.roots)).toBe(true);
      expect(jestConfig.roots).toEqual(['<rootDir>/src']);
      expect(jestConfig.roots.length).toBe(1);
    });
  });

  describe('Test Matching Rules and Invariants', () => {
    it('should define deterministic testMatch patterns', () => {
      expect(Array.isArray(jestConfig.testMatch)).toBe(true);
      expect(jestConfig.testMatch).toContain('**/__tests__/**/*.ts');
      expect(jestConfig.testMatch).toContain('**/?(*.)+(spec|test).ts');
    });

    it('should properly transform TypeScript files with ts-jest', () => {
      expect(jestConfig.transform).toBeDefined();
      expect(typeof jestConfig.transform).toBe('object');
      expect(jestConfig.transform['^.+\\.ts$']).toBe('ts-jest');
    });
  });

  describe('Coverage Collection Rules and Exclusions', () => {
    it('should include src/**/*.ts in coverage collection', () => {
      expect(Array.isArray(jestConfig.collectCoverageFrom)).toBe(true);
      expect(jestConfig.collectCoverageFrom).toContain('src/**/*.ts');
    });

    it('should exclude test files and entrypoints from coverage collection', () => {
      const exclusions = jestConfig.collectCoverageFrom.filter((pattern: string) =>
        pattern.startsWith('!')
      );

      expect(exclusions).toContain('!src/**/*.test.ts');
      expect(exclusions).toContain('!src/**/*.spec.ts');
      expect(exclusions).toContain('!src/index.ts');
    });
  });

  describe('Coverage Threshold Enforcement and Boundary Conditions', () => {
    it('should enforce strict global coverage thresholds at 95%', () => {
      expect(jestConfig.coverageThreshold).toBeDefined();
      expect(jestConfig.coverageThreshold.global).toBeDefined();

      const { branches, functions, lines, statements } = jestConfig.coverageThreshold.global;

      expect(branches).toBe(95);
      expect(functions).toBe(95);
      expect(lines).toBe(95);
      expect(statements).toBe(95);
    });

    it('should validate all threshold boundary values are between 0 and 100', () => {
      const thresholds = jestConfig.coverageThreshold.global;
      for (const [metric, value] of Object.entries(thresholds)) {
        expect(typeof value).toBe('number');
        expect(Number.isFinite(value)).toBe(true);
        expect(value).toBeGreaterThanOrEqual(0);
        expect(value).toBeLessThanOrEqual(100);
      }
    });
  });

  describe('Immutability and Determinism', () => {
    it('should produce identical configuration across repeated imports', () => {
      const config1 = require(configPath);
      jest.resetModules();
      const config2 = require(configPath);

      expect(config1).toEqual(config2);
      expect(config1).not.toBe(config2); // New reference on fresh module load
    });

    it('should reject invalid or mutated threshold states in validation helper', () => {
      const validateThreshold = (threshold: number) => {
        if (typeof threshold !== 'number' || isNaN(threshold)) {
          throw new Error('Threshold must be a valid number');
        }
        if (threshold < 0 || threshold > 100) {
          throw new RangeError('Threshold must be between 0 and 100');
        }
        return true;
      };

      expect(() => validateThreshold(jestConfig.coverageThreshold.global.lines)).not.toThrow();
      expect(() => validateThreshold(-1)).toThrow(RangeError);
      expect(() => validateThreshold(101)).toThrow(RangeError);
      expect(() => validateThreshold(NaN)).toThrow(Error);
    });
  });
});
