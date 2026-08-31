/**
 * Oracle Telemetry
 *
 * Tracks latency, failure, and recovery signals across update cycles.
 * All emitted data is safe for structured logging — no secrets, keys, or
 * raw RPC messages are ever stored or surfaced here.
 */

import type { UpdateCycleDiagnostics, ProviderFetchEvent } from '../types/index.js';
import { logger } from '../utils/logger.js';

// ─── Per-provider rolling counters ────────────────────────────────────────────

interface ProviderStats {
    successCount: number;
    failureCount: number;
    /** Running sum of latency for successful fetches (ms). */
    totalSuccessLatencyMs: number;
    /** Running sum of latency for failed fetches (ms). */
    totalFailureLatencyMs: number;
    /** Unix timestamp (ms) of the most-recent successful fetch. */
    lastSuccessAt: number;
    /** Unix timestamp (ms) of the most-recent failure. */
    lastFailureAt: number;
    /** Most-recent error class seen from this provider. */
    lastErrorClass: ProviderFetchEvent['errorClass'];
}

function emptyStats(): ProviderStats {
    return {
        successCount: 0,
        failureCount: 0,
        totalSuccessLatencyMs: 0,
        totalFailureLatencyMs: 0,
        lastSuccessAt: 0,
        lastFailureAt: 0,
        lastErrorClass: undefined,
    };
}

// ─── OracleTelemetry ──────────────────────────────────────────────────────────

/**
 * Lightweight in-process telemetry collector for the oracle update loop.
 *
 * Call `recordCycle` at the end of every update cycle. The class maintains
 * rolling per-provider counters and exposes `getSummary` for health-check
 * endpoints or structured log emission.
 *
 * Thread-safety: single-threaded Node.js event loop — no locking required.
 */
export class OracleTelemetry {
    private cycleCount = 0;
    private totalCycleDurationMs = 0;
    private lastCycleAt = 0;
    private providerStats = new Map<string, ProviderStats>();

    /**
     * Record the outcome of one completed update cycle.
     *
     * @param diagnostics  Structured diagnostics produced by `OracleService.updatePrices`.
     */
    recordCycle(diagnostics: UpdateCycleDiagnostics): void {
        this.cycleCount += 1;
        this.totalCycleDurationMs += diagnostics.durationMs;
        this.lastCycleAt = Date.now();

        for (const event of diagnostics.providerEvents) {
            let stats = this.providerStats.get(event.provider);
            if (!stats) {
                stats = emptyStats();
                this.providerStats.set(event.provider, stats);
            }

            if (event.success) {
                stats.successCount += 1;
                stats.totalSuccessLatencyMs += event.latencyMs;
                stats.lastSuccessAt = Date.now();
            } else {
                stats.failureCount += 1;
                stats.totalFailureLatencyMs += event.latencyMs;
                stats.lastFailureAt = Date.now();
                stats.lastErrorClass = event.errorClass;
            }
        }

        // Emit a structured diagnostic log so operators can grep / forward to
        // observability tools without any further processing.
        logger.info('oracle.cycle', {
            cycleCount: this.cycleCount,
            durationMs: diagnostics.durationMs,
            assetsUpdated: diagnostics.assetsUpdated,
            assetsFailed: diagnostics.assetsFailed,
            contractUpdateOk: diagnostics.contractUpdateOk,
            cooledDownProviders: diagnostics.cooledDownProviders,
        });

        if (diagnostics.assetsFailed > 0 || !diagnostics.contractUpdateOk) {
            logger.warn('oracle.cycle.degraded', {
                assetsFailed: diagnostics.assetsFailed,
                contractUpdateOk: diagnostics.contractUpdateOk,
                failedEvents: diagnostics.providerEvents
                    .filter((e) => !e.success)
                    .map((e) => ({ provider: e.provider, asset: e.asset, errorClass: e.errorClass })),
            });
        }
    }

    /**
     * Return a snapshot of aggregated telemetry, safe for external consumption
     * (health-check endpoint, admin API, structured log line).
     */
    getSummary(): TelemetrySummary {
        const avgCycleDurationMs =
            this.cycleCount > 0 ? Math.round(this.totalCycleDurationMs / this.cycleCount) : 0;

        const providers: Record<string, ProviderSummary> = {};
        for (const [name, s] of this.providerStats) {
            const totalFetches = s.successCount + s.failureCount;
            providers[name] = {
                successCount: s.successCount,
                failureCount: s.failureCount,
                successRate: totalFetches > 0 ? s.successCount / totalFetches : 1,
                avgSuccessLatencyMs:
                    s.successCount > 0 ? Math.round(s.totalSuccessLatencyMs / s.successCount) : 0,
                lastSuccessAt: s.lastSuccessAt > 0 ? new Date(s.lastSuccessAt).toISOString() : null,
                lastFailureAt: s.lastFailureAt > 0 ? new Date(s.lastFailureAt).toISOString() : null,
                lastErrorClass: s.lastErrorClass ?? null,
            };
        }

        return {
            cycleCount: this.cycleCount,
            avgCycleDurationMs,
            lastCycleAt: this.lastCycleAt > 0 ? new Date(this.lastCycleAt).toISOString() : null,
            providers,
        };
    }

    /** Reset all counters (e.g. after a config reload). */
    reset(): void {
        this.cycleCount = 0;
        this.totalCycleDurationMs = 0;
        this.lastCycleAt = 0;
        this.providerStats.clear();
    }
}

// ─── Public summary types ─────────────────────────────────────────────────────

export interface ProviderSummary {
    successCount: number;
    failureCount: number;
    /** Fraction of fetches that returned a valid price [0, 1]. */
    successRate: number;
    avgSuccessLatencyMs: number;
    lastSuccessAt: string | null;
    lastFailureAt: string | null;
    lastErrorClass: ProviderFetchEvent['errorClass'] | null;
}

export interface TelemetrySummary {
    cycleCount: number;
    avgCycleDurationMs: number;
    lastCycleAt: string | null;
    providers: Record<string, ProviderSummary>;
}
