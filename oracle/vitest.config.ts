import { defineConfig } from 'vitest/config';

import { resolve } from 'node:path';

/**
 * Validation invariants:
 * - Thresholds must be finite numbers in [0, 100].
 * - Coverage include globs must be non-empty strings.
 * - Test include globs must be non-empty strings.
 * - Reporters must be a non-empty array of non-empty strings.
 * - The config must be deterministic: the same inputs always yield the same output.
 */

const DEFAULT_TEST_INCLUDE = ['tests/**/*.test.ts'] as const;
const DEFAULT_COVERAGE_INCLUDE = ['src/**/*.ts'] as const;
const DEFAULT_COVERAGE_EXCLUDE = ['src/index.ts'] as const;
const DEFAULT_REPORTERS = ['text', 'html', 'lcovc'] as const;
const DEFAULT_THRESHOLDS = {
    lines: 95,
    functions: 95,
    branches: 90,
    statements: 95,
} as const;

export type CoverageThresholds = {
    lines: number;
    functions: number;
    branches: number;
    statements: number;
};

export type VitestOracleConfig = {
    test: {
        globals: boolean;
        environment: 'node';
        include: string[];
        coverage: {
            provider: 'v8';
            reporter: string[];
            include: string[];
            exclude: string[];
            thresholds: CoverageThresholds;
        };
    };
};

function assertNonEmptyStringArray(name: string, value: unknown): asserts value is string[] {
    if (!Array.isArray(value) || value.length === 0) {
        throw new Error(`vitest config invariant violated: ${name} must be a non-empty array`);
    }
    for (const entry of value) {
        if (typeof entry !== 'string' || entry.trim().length === 0) {
            throw new Error(
                `vitest config invariant violated: ${name} must contain only non-empty strings`,
            );
        }
    }
}

function assertThreshold(name: string, value: unknown): asserts value is number {
    if (typeof value !== 'number' || !Number.isFinite(value)) {
        throw new Error(`vitest config invariant violated: thresholds.${name} must be a finite number`);
    }
    if (value < 0 || value > 100) {
        throw new Error(
            `vitest config invariant violated: thresholds.${name} must be within [0, 100], received ${value}`,
        );
    }
}

/**
 * Build a deterministic, validated Vitest config for the oracle package.
 *
 * This function is exported so tests can exercise success, rejection,
 * boundary, and regression scenarios without mutating the module singleton.
 * It never mutates its inputs and always returns a fresh object.
 */
export function buildOracleVitestConfig(
    overrides: Partial<VitestOracleConfig['test']> = {},
): VitestOracleConfig {
    const testInclude = overrides.include ?? [...DEFAULT_TEST_INCLUDE ];
    const coverageInclude = overrides.coverage?.include ?? [...DEFAULT_COVERAGE_INCLUDE];
    const coverageExclude = overrides.coverage?.exclude ?? [...DEFAULT_COVERAGE_EXCLUDE];
    const reporters = overrides.coverage?.reporter ?? [...DEFAULT_REPORTERS];
    const thresholds = {
        ...DEFAULT_THRESHOLDS,
        ...(overrides.coverage?.thresholds ?? {}),
    };

    assertNonEmptyStringArray('test.include', testInclude);
    assertNonEmptyStringArray('coverage.include', coverageInclude);
    assertNonEmptyStringArray('coverage.reporter', reporters);
    // Exclude may be empty, but if present every entry must be a non-empty string.
    if (coverageExclude.length > 0) {
        assertNonEmptyStringArray('coverage.exclude', coverageExclude);
    }
    assertThreshold('lines', thresholds.lines);
    assertThreshold('functions', thresholds.functions);
    assertThreshold('branches', thresholds.branches);
    assertThreshold('statements', thresholds.statements);

    return {
        test: {
            globals: overrides.globals ?? false,
            environment: 'node',
            include: [...testInclude],
            coverage: {
                provider: 'v8',
                reporter: [...reporters],
                include: [...coverageInclude],
                exclude: [...coverageExclude],
                thresholds: { ...thresholds },
            },
        },
    };
}

/**
 * Default export consumed by Vitest. Built from the validated factory so
 * invalid defaults fail fast at module load time rather than silently producing
 * an undefined coverage policy.
 */
export default defineConfig(buildOracleVitestConfig());
