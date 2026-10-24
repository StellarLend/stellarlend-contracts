/**
 * Failure-Path and Boundary Coverage for contract-updater.ts
 *
 * Maps to invariants documented in the source module:
 *   I-1  Input validation – empty asset, oversized asset, zero/negative price, bad timestamp
 *   I-2  Secret-key sanitization – never appears in error messages or logs
 *   I-3  Retry loop – exponential back-off, exhaustion after maxRetries
 *   I-4  Transaction-poll timeout – bounded loop, does not spin forever
 *   I-5  Batch failure isolation – one bad asset does not abort the rest
 *
 * Design notes:
 *  - vi.mock() factory is only called ONCE (at module load); it must not be
 *    cleared or restored by afterEach/vi.restoreAllMocks because that would
 *    erase the Address/Contract implementations.
 *  - The `mockServer` variable is refreshed in beforeEach by assigning a new
 *    plain-object literal.  The Server constructor closure reads the outer
 *    binding so every `new SorobanRpc.Server()` call sees the new object.
 *  - Api helper return values (isSimulationError/isSimulationSuccess) are reset
 *    to happy-path defaults in beforeEach via vi.mocked().mockReturnValue().
 */

import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  ContractUpdater,
  createContractUpdater,
  MAX_ASSET_SYMBOL_BYTES,
  DEFAULT_TX_POLL_TIMEOUT_MS,
} from "../src/services/contract-updater.js";
import type { AggregatedPrice } from "../src/types/index.js";

// ---------------------------------------------------------------------------
// Mutable mock-server factory
//
// We reassign `mockServer` in beforeEach so each test starts from a clean,
// "all-happy" state.  The SorobanRpc.Server mock factory uses the outer
// reference, so the reassignment is immediately visible.
// ---------------------------------------------------------------------------
let mockServer = {
  getAccount: vi.fn(),
  simulateTransaction: vi.fn(),
  sendTransaction: vi.fn(),
  getTransaction: vi.fn(),
};

// Fixed mock transaction / builder used across all tests.
const _mockTransaction = {
  sign: vi.fn(),
  toXDR: vi.fn().mockReturnValue("mock-xdr"),
};

const _mockTransactionBuilder = {
  addOperation: vi.fn().mockReturnThis(),
  setTimeout: vi.fn().mockReturnThis(),
  build: vi.fn().mockReturnValue(_mockTransaction),
};

// ---------------------------------------------------------------------------
// Module mock
// ---------------------------------------------------------------------------
vi.mock("@stellar/stellar-sdk", () => ({
  Keypair: {
    fromSecret: vi.fn((secret: string) => ({
      publicKey: () => "GADMIN_PUBLIC_KEY_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
      secret: () => secret,
    })),
  },
  Contract: vi.fn().mockImplementation(() => ({
    call: vi.fn().mockReturnValue({}),
  })),
  SorobanRpc: {
    Server: vi.fn().mockImplementation(() => mockServer),
    Api: {
      isSimulationError: vi.fn().mockReturnValue(false),
      isSimulationSuccess: vi.fn().mockReturnValue(true),
      GetTransactionStatus: {
        SUCCESS: "SUCCESS",
        FAILED: "FAILED",
        NOT_FOUND: "NOT_FOUND",
      },
    },
    assembleTransaction: vi.fn().mockImplementation(() => ({
      build: () => _mockTransaction,
    })),
  },
  TransactionBuilder: vi.fn().mockImplementation(() => _mockTransactionBuilder),
  Networks: {
    TESTNET: "Test SDF Network ; September 2015",
    PUBLIC: "Public Global Stellar Network ; September 2015",
  },
  xdr: {
    ScVal: {
      scvSymbol: vi.fn((s: string) => ({ symbol: s })),
    },
  },
  Address: vi.fn().mockImplementation((addr: string) => ({
    toScVal: vi.fn().mockReturnValue({ address: addr }),
  })),
  nativeToScVal: vi.fn((v: unknown, o: unknown) => ({ v, o })),
}));

// ---------------------------------------------------------------------------
// Reset helpers
// ---------------------------------------------------------------------------

/** Reset the mock server to default "all-succeed" behaviour. */
function resetMockServer() {
  mockServer.getAccount.mockReset().mockResolvedValue({
    accountId: () => "GTEST123",
    sequenceNumber: () => "1",
    incrementSequenceNumber: vi.fn(),
  });
  mockServer.simulateTransaction.mockReset().mockResolvedValue({
    results: [{ xdr: "mock-xdr" }],
  });
  mockServer.sendTransaction.mockReset().mockResolvedValue({
    status: "PENDING",
    hash: "mock-tx-hash-123456",
  });
  mockServer.getTransaction.mockReset().mockResolvedValue({
    status: "SUCCESS",
  });
}

async function resetApiMocks() {
  const sdk = await import("@stellar/stellar-sdk");
  vi.mocked(sdk.SorobanRpc.Api.isSimulationError).mockReturnValue(false);
  vi.mocked(sdk.SorobanRpc.Api.isSimulationSuccess).mockReturnValue(true);
}

// ---------------------------------------------------------------------------
// Fixture helpers
// ---------------------------------------------------------------------------

const BASE_CONFIG = {
  network: "testnet" as const,
  rpcUrl: "https://soroban-testnet.stellar.org",
  contractId: "CTEST123456789",
  adminSecretKey: "STEST_SECRET_KEY_ABCDEFGHIJKLMNOPQRSTUVWXYZ01234",
  maxRetries: 3,
  retryDelayMs: 10,
  txPollTimeoutMs: 500,
};

function makeUpdater(
  overrides: Partial<typeof BASE_CONFIG> = {},
): ContractUpdater {
  return createContractUpdater({ ...BASE_CONFIG, ...overrides });
}

function makeAggregatedPrice(
  asset: string,
  price: bigint,
  timestamp: number,
): AggregatedPrice {
  return { asset, price, timestamp, sources: [], confidence: 95 };
}

// ---------------------------------------------------------------------------
// Suite
// ---------------------------------------------------------------------------

describe("ContractUpdater – Failure-Path & Boundary Coverage", () => {
  beforeEach(async () => {
    resetMockServer();
    await resetApiMocks();
  });

  // NOTE: No afterEach with vi.restoreAllMocks() – that would erase the
  // Address / Contract mock implementations defined in vi.mock().

  // =========================================================================
  // I-1  Input Validation
  // =========================================================================

  describe("I-1: Input Validation", () => {
    describe("asset parameter", () => {
      it("rejects an empty string asset", async () => {
        const updater = makeUpdater();
        const result = await updater.updatePrice("", 100_000n, 1_700_000_000);

        expect(result.success).toBe(false);
        expect(result.error).toMatch(/non-empty string/i);
      });

      it("rejects a whitespace-only asset", async () => {
        const updater = makeUpdater();
        const result = await updater.updatePrice(
          "   ",
          100_000n,
          1_700_000_000,
        );

        expect(result.success).toBe(false);
        expect(result.error).toMatch(/non-empty string/i);
      });

      it("rejects an asset symbol that exceeds MAX_ASSET_SYMBOL_BYTES", async () => {
        const oversizedAsset = "A".repeat(MAX_ASSET_SYMBOL_BYTES + 1);
        const updater = makeUpdater();
        const result = await updater.updatePrice(
          oversizedAsset,
          100_000n,
          1_700_000_000,
        );

        expect(result.success).toBe(false);
        expect(result.error).toMatch(/maximum length/i);
      });

      it("accepts an asset symbol exactly at MAX_ASSET_SYMBOL_BYTES", async () => {
        const exactAsset = "A".repeat(MAX_ASSET_SYMBOL_BYTES);
        const updater = makeUpdater();
        const result = await updater.updatePrice(
          exactAsset,
          100_000n,
          1_700_000_000,
        );

        // Must NOT fail due to length.
        if (!result.success) {
          expect(result.error).not.toMatch(/maximum length/i);
        }
      });

      it("does not make any network call when asset is invalid", async () => {
        const updater = makeUpdater();
        await updater.updatePrice("", 100_000n, 1_700_000_000);

        expect(mockServer.getAccount).not.toHaveBeenCalled();
        expect(mockServer.simulateTransaction).not.toHaveBeenCalled();
        expect(mockServer.sendTransaction).not.toHaveBeenCalled();
      });
    });

    describe("price parameter", () => {
      it("rejects price === 0n", async () => {
        const updater = makeUpdater();
        const result = await updater.updatePrice("XLM", 0n, 1_700_000_000);

        expect(result.success).toBe(false);
        expect(result.error).toMatch(/positive bigint/i);
      });

      it("rejects negative price (-1n)", async () => {
        const updater = makeUpdater();
        const result = await updater.updatePrice("XLM", -1n, 1_700_000_000);

        expect(result.success).toBe(false);
        expect(result.error).toMatch(/positive bigint/i);
      });

      it("rejects very large negative price", async () => {
        const updater = makeUpdater();
        const result = await updater.updatePrice(
          "XLM",
          -999_999_999_999n,
          1_700_000_000,
        );

        expect(result.success).toBe(false);
        expect(result.error).toMatch(/positive bigint/i);
      });

      it("accepts price === 1n (minimum valid positive)", async () => {
        const updater = makeUpdater();
        const result = await updater.updatePrice("XLM", 1n, 1_700_000_000);

        if (!result.success) {
          expect(result.error).not.toMatch(/positive bigint/i);
        }
      });

      it("accepts maximum realistic price value", async () => {
        const maxPrice = 999_999_999_999_999_999n;
        const updater = makeUpdater();
        const result = await updater.updatePrice(
          "BTC",
          maxPrice,
          1_700_000_000,
        );

        if (!result.success) {
          expect(result.error).not.toMatch(/positive bigint/i);
        }
        expect(result.price).toBe(maxPrice);
      });

      it("does not make any network call when price is invalid", async () => {
        const updater = makeUpdater();
        await updater.updatePrice("XLM", 0n, 1_700_000_000);

        expect(mockServer.getAccount).not.toHaveBeenCalled();
      });
    });

    describe("timestamp parameter", () => {
      it("rejects timestamp === 0", async () => {
        const updater = makeUpdater();
        const result = await updater.updatePrice("XLM", 100_000n, 0);

        expect(result.success).toBe(false);
        expect(result.error).toMatch(/timestamp/i);
      });

      it("rejects negative timestamp", async () => {
        const updater = makeUpdater();
        const result = await updater.updatePrice("XLM", 100_000n, -1);

        expect(result.success).toBe(false);
        expect(result.error).toMatch(/timestamp/i);
      });

      it("rejects NaN timestamp", async () => {
        const updater = makeUpdater();
        const result = await updater.updatePrice("XLM", 100_000n, NaN);

        expect(result.success).toBe(false);
        expect(result.error).toMatch(/timestamp/i);
      });

      it("rejects Infinity timestamp", async () => {
        const updater = makeUpdater();
        const result = await updater.updatePrice("XLM", 100_000n, Infinity);

        expect(result.success).toBe(false);
        expect(result.error).toMatch(/timestamp/i);
      });

      it("rejects non-integer timestamp (float)", async () => {
        const updater = makeUpdater();
        const result = await updater.updatePrice(
          "XLM",
          100_000n,
          1_700_000_000.5,
        );

        expect(result.success).toBe(false);
        expect(result.error).toMatch(/timestamp/i);
      });

      it("accepts a current epoch timestamp", async () => {
        const ts = Math.floor(Date.now() / 1000);
        const updater = makeUpdater();
        const result = await updater.updatePrice("XLM", 100_000n, ts);

        if (!result.success) {
          expect(result.error).not.toMatch(/timestamp/i);
        }
      });

      it("does not make any network call when timestamp is invalid", async () => {
        const updater = makeUpdater();
        await updater.updatePrice("XLM", 100_000n, 0);

        expect(mockServer.getAccount).not.toHaveBeenCalled();
      });
    });

    describe("result shape for invalid inputs", () => {
      it("returns the original asset/price/timestamp even on validation failure", async () => {
        const updater = makeUpdater();
        const result = await updater.updatePrice("", 0n, -1);

        expect(result.asset).toBe("");
        expect(result.price).toBe(0n);
        expect(result.timestamp).toBe(-1);
        expect(result.success).toBe(false);
        expect(result.error).toBeDefined();
      });
    });
  });

  // =========================================================================
  // I-2  Secret-Key Sanitization
  // =========================================================================

  describe("I-2: Secret-Key Sanitization", () => {
    it("does not expose the secret key via getAdminPublicKey()", () => {
      const updater = makeUpdater();
      const publicKey = updater.getAdminPublicKey();

      expect(publicKey).not.toContain(BASE_CONFIG.adminSecretKey);
    });

    it("redacts the secret key when it appears in an error message", async () => {
      mockServer.getAccount.mockRejectedValue(
        new Error(`auth rejected for key ${BASE_CONFIG.adminSecretKey}`),
      );

      const updater = makeUpdater({ maxRetries: 1 });
      const result = await updater.updatePrice("XLM", 100_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(result.error).not.toContain(BASE_CONFIG.adminSecretKey);
      expect(result.error).toContain("[REDACTED]");
    });

    it("does not expose the secret key when all retries are exhausted", async () => {
      mockServer.getAccount.mockRejectedValue(
        new Error(`key=${BASE_CONFIG.adminSecretKey} not found on ledger`),
      );

      const updater = makeUpdater({ maxRetries: 2 });
      const result = await updater.updatePrice("XLM", 100_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(result.error).not.toContain(BASE_CONFIG.adminSecretKey);
      expect(result.error).toContain("[REDACTED]");
    });
  });

  // =========================================================================
  // I-3  Retry Loop – Exponential Back-Off & Exhaustion
  // =========================================================================

  describe("I-3: Retry Loop", () => {
    it("succeeds on the second attempt after a transient failure", async () => {
      let calls = 0;
      mockServer.simulateTransaction.mockImplementation(async () => {
        calls++;
        if (calls === 1) throw new Error("transient network error");
        return { results: [{ xdr: "ok" }] };
      });

      const updater = makeUpdater({ maxRetries: 3 });
      const result = await updater.updatePrice("XLM", 100_000n, 1_700_000_000);

      expect(result.success).toBe(true);
      expect(calls).toBe(2);
    });

    it("returns failure after all maxRetries are exhausted", async () => {
      mockServer.simulateTransaction.mockRejectedValue(
        new Error("persistent network failure"),
      );

      const updater = makeUpdater({ maxRetries: 3 });
      const result = await updater.updatePrice("XLM", 100_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(result.error).toMatch(/persistent network failure/i);
      expect(mockServer.simulateTransaction).toHaveBeenCalledTimes(3);
    });

    it("makes exactly 1 attempt when maxRetries === 1", async () => {
      mockServer.simulateTransaction.mockRejectedValue(new Error("fail"));

      const updater = makeUpdater({ maxRetries: 1 });
      await updater.updatePrice("XLM", 100_000n, 1_700_000_000);

      expect(mockServer.simulateTransaction).toHaveBeenCalledTimes(1);
    });

    it("applies exponential back-off – measured delays increase geometrically", async () => {
      let call = 0;
      mockServer.simulateTransaction.mockImplementation(async () => {
        call++;
        if (call < 3) throw new Error("retry");
        return { results: [{ xdr: "ok" }] };
      });

      const recordedDelays: number[] = [];
      const originalSetTimeout = globalThis.setTimeout;

      // Intercept setTimeout to capture requested back-off delays.
      const spy = vi
        .spyOn(globalThis, "setTimeout")
        .mockImplementation(
          (fn: TimerHandler, delay?: number, ...args: unknown[]) => {
            const RETRY_DELAY_MS = 50;
            if (typeof delay === "number" && delay >= RETRY_DELAY_MS) {
              recordedDelays.push(delay);
            }
            // Collapse delay to 0 so the test runs fast.
            return originalSetTimeout(fn as () => void, 0, ...args);
          },
        );

      const updater = makeUpdater({ maxRetries: 3, retryDelayMs: 50 });
      const result = await updater.updatePrice("XLM", 100_000n, 1_700_000_000);

      spy.mockRestore();

      expect(result.success).toBe(true);
      // First back-off 50 ms (2^0), second 100 ms (2^1): delays[1] > delays[0].
      expect(recordedDelays.length).toBeGreaterThanOrEqual(1);
      if (recordedDelays.length >= 2) {
        expect(recordedDelays[1]).toBeGreaterThan(recordedDelays[0]);
      }
    });

    it("never throws – always resolves to a ContractUpdateResult", async () => {
      mockServer.getAccount.mockRejectedValue(new Error("total meltdown"));

      const updater = makeUpdater({ maxRetries: 2 });
      await expect(
        updater.updatePrice("BTC", 50_000_000_000n, 1_700_000_000),
      ).resolves.toMatchObject({ success: false });
    });

    it("non-Error rejection is coerced to a string error message", async () => {
      mockServer.simulateTransaction.mockRejectedValue("raw string rejection");

      const updater = makeUpdater({ maxRetries: 1 });
      const result = await updater.updatePrice("XLM", 100_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(typeof result.error).toBe("string");
      expect(result.error!.length).toBeGreaterThan(0);
    });
  });

  // =========================================================================
  // I-4  Transaction-Poll Timeout
  // =========================================================================

  describe("I-4: Transaction-Poll Timeout", () => {
    it("returns failure when the poll loop exceeds txPollTimeoutMs", async () => {
      mockServer.getTransaction.mockResolvedValue({ status: "NOT_FOUND" });

      const updater = makeUpdater({
        maxRetries: 1,
        retryDelayMs: 5,
        txPollTimeoutMs: 50,
      });

      const result = await updater.updatePrice("XLM", 100_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(result.error).toMatch(/timed out/i);
    });

    it("succeeds when the tx confirms before txPollTimeoutMs", async () => {
      mockServer.getTransaction
        .mockResolvedValueOnce({ status: "NOT_FOUND" })
        .mockResolvedValueOnce({ status: "SUCCESS" });

      const updater = makeUpdater({ txPollTimeoutMs: 10_000 });
      const result = await updater.updatePrice("XLM", 100_000n, 1_700_000_000);

      expect(result.success).toBe(true);
    });

    it("DEFAULT_TX_POLL_TIMEOUT_MS is exported and equals 60 000 ms", () => {
      expect(DEFAULT_TX_POLL_TIMEOUT_MS).toBe(60_000);
    });

    it("falls back to DEFAULT_TX_POLL_TIMEOUT_MS when txPollTimeoutMs is omitted", () => {
      const { txPollTimeoutMs: _omitted, ...configWithoutPoll } = BASE_CONFIG;
      void _omitted;
      const updater = createContractUpdater(configWithoutPoll as any);
      expect(updater).toBeInstanceOf(ContractUpdater);
    });

    it("poll-timeout error message does not contain the secret key", async () => {
      mockServer.getTransaction.mockResolvedValue({ status: "NOT_FOUND" });

      const updater = makeUpdater({
        maxRetries: 1,
        txPollTimeoutMs: 30,
      });
      const result = await updater.updatePrice("XLM", 100_000n, 1_700_000_000);

      expect(result.error).not.toContain(BASE_CONFIG.adminSecretKey);
    });
  });

  // =========================================================================
  // I-5  Batch Failure Isolation (updatePrices)
  // =========================================================================

  describe("I-5: Batch Failure Isolation", () => {
    it("continues processing remaining assets after one fails a network call", async () => {
      let call = 0;
      mockServer.simulateTransaction.mockImplementation(async () => {
        call++;
        if (call === 1) throw new Error("first asset fails");
        return { results: [{ xdr: "ok" }] };
      });

      const prices: AggregatedPrice[] = [
        makeAggregatedPrice("FAILASSET", 100n, 1_700_000_000),
        makeAggregatedPrice("BTC", 50_000_000_000n, 1_700_000_000),
      ];

      const updater = makeUpdater({ maxRetries: 1 });
      const results = await updater.updatePrices(prices);

      expect(results).toHaveLength(2);
      expect(results[0].success).toBe(false);
      expect(results[0].asset).toBe("FAILASSET");
      expect(results[1].success).toBe(true);
      expect(results[1].asset).toBe("BTC");
    });

    it("returns all successes when no assets fail", async () => {
      const prices: AggregatedPrice[] = [
        makeAggregatedPrice("XLM", 150_000n, 1_700_000_000),
        makeAggregatedPrice("BTC", 50_000_000_000n, 1_700_000_000),
        makeAggregatedPrice("ETH", 3_000_000_000n, 1_700_000_000),
      ];

      const updater = makeUpdater();
      const results = await updater.updatePrices(prices);

      expect(results).toHaveLength(3);
      results.forEach((r) => expect(r.success).toBe(true));
    });

    it("returns all failures when every network call fails", async () => {
      mockServer.simulateTransaction.mockRejectedValue(new Error("rpc down"));

      const prices: AggregatedPrice[] = [
        makeAggregatedPrice("XLM", 150_000n, 1_700_000_000),
        makeAggregatedPrice("BTC", 50_000_000_000n, 1_700_000_000),
      ];

      const updater = makeUpdater({ maxRetries: 1 });
      const results = await updater.updatePrices(prices);

      expect(results).toHaveLength(2);
      results.forEach((r) => expect(r.success).toBe(false));
    });

    it("handles an empty price array and returns an empty results array", async () => {
      const updater = makeUpdater();
      const results = await updater.updatePrices([]);

      expect(results).toHaveLength(0);
    });

    it("validates each asset individually – invalid input does not abort batch", async () => {
      const prices: AggregatedPrice[] = [
        makeAggregatedPrice("XLM", 150_000n, 1_700_000_000),
        makeAggregatedPrice("BAD", 0n, 1_700_000_000), // price = 0 fails I-1
        makeAggregatedPrice("BTC", 50_000_000_000n, 1_700_000_000),
      ];

      const updater = makeUpdater();
      const results = await updater.updatePrices(prices);

      expect(results).toHaveLength(3);
      expect(results[0].success).toBe(true); // XLM ok
      expect(results[1].success).toBe(false); // BAD rejected by I-1
      expect(results[1].error).toMatch(/positive bigint/i);
      expect(results[2].success).toBe(true); // BTC ok
    });

    it("preserves result order matching the input order", async () => {
      const assets = ["XLM", "BTC", "ETH", "USDC", "USDT"];
      const prices = assets.map((a) =>
        makeAggregatedPrice(a, 100_000n, 1_700_000_000),
      );

      const updater = makeUpdater();
      const results = await updater.updatePrices(prices);

      results.forEach((r, i) => {
        expect(r.asset).toBe(assets[i]);
      });
    });
  });

  // =========================================================================
  // Network Error Scenarios
  // =========================================================================

  describe("Network Error Scenarios", () => {
    it("returns failure when getAccount throws", async () => {
      mockServer.getAccount.mockRejectedValue(new Error("Account not found"));

      const updater = makeUpdater({ maxRetries: 1 });
      const result = await updater.updatePrice("XLM", 150_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(result.error).toMatch(/Account not found/i);
    });

    it("returns failure when simulateTransaction returns an error response", async () => {
      const sdk = await import("@stellar/stellar-sdk");
      vi.mocked(sdk.SorobanRpc.Api.isSimulationError).mockReturnValue(true);

      const updater = makeUpdater({ maxRetries: 1 });
      const result = await updater.updatePrice("XLM", 150_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(result.error).toMatch(/Simulation failed/i);
    });

    it("returns failure when simulation is neither error nor success", async () => {
      const sdk = await import("@stellar/stellar-sdk");
      vi.mocked(sdk.SorobanRpc.Api.isSimulationError).mockReturnValue(false);
      vi.mocked(sdk.SorobanRpc.Api.isSimulationSuccess).mockReturnValue(false);

      const updater = makeUpdater({ maxRetries: 1 });
      const result = await updater.updatePrice("XLM", 150_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(result.error).toMatch(/did not succeed/i);
    });

    it("returns failure when sendTransaction returns ERROR status", async () => {
      mockServer.sendTransaction.mockResolvedValue({
        status: "ERROR",
        errorResult: "op_bad_auth",
        hash: "",
      });

      const updater = makeUpdater({ maxRetries: 1 });
      const result = await updater.updatePrice("XLM", 150_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(result.error).toMatch(/Transaction failed/i);
    });

    it("returns failure when the on-chain transaction is FAILED", async () => {
      mockServer.getTransaction.mockResolvedValue({ status: "FAILED" });

      const updater = makeUpdater({ maxRetries: 1 });
      const result = await updater.updatePrice("XLM", 150_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(result.error).toMatch(/failed on-chain/i);
    });

    it("returns failure when sendTransaction throws ECONNREFUSED", async () => {
      mockServer.sendTransaction.mockRejectedValue(
        new Error("connect ECONNREFUSED 127.0.0.1:8000"),
      );

      const updater = makeUpdater({ maxRetries: 1 });
      const result = await updater.updatePrice("XLM", 150_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(result.error).toMatch(/ECONNREFUSED/i);
    });
  });

  // =========================================================================
  // Simulation Error Branches (individual code-path coverage)
  // =========================================================================

  describe("Simulation Error Branches", () => {
    it('isSimulationError branch: error contains "Simulation failed"', async () => {
      const sdk = await import("@stellar/stellar-sdk");
      vi.mocked(sdk.SorobanRpc.Api.isSimulationError).mockReturnValue(true);
      mockServer.simulateTransaction.mockResolvedValue({
        error: "contract panic",
      });

      const updater = makeUpdater({ maxRetries: 1 });
      const result = await updater.updatePrice("XLM", 150_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(result.error).toMatch(/Simulation failed/i);
    });

    it('!isSimulationSuccess branch: error contains "did not succeed"', async () => {
      const sdk = await import("@stellar/stellar-sdk");
      vi.mocked(sdk.SorobanRpc.Api.isSimulationError).mockReturnValue(false);
      vi.mocked(sdk.SorobanRpc.Api.isSimulationSuccess).mockReturnValue(false);

      const updater = makeUpdater({ maxRetries: 1 });
      const result = await updater.updatePrice("XLM", 150_000n, 1_700_000_000);

      expect(result.success).toBe(false);
      expect(result.error).toMatch(/did not succeed/i);
    });
  });

  // =========================================================================
  // Success Path Invariants
  // =========================================================================

  describe("Success Path Invariants", () => {
    it("returns transactionHash on success", async () => {
      const updater = makeUpdater();
      const result = await updater.updatePrice("XLM", 150_000n, 1_700_000_000);

      expect(result.success).toBe(true);
      expect(result.transactionHash).toBe("mock-tx-hash-123456");
    });

    it("echoes back the correct asset, price, and timestamp", async () => {
      const updater = makeUpdater();
      const ts = 1_700_000_000;
      const result = await updater.updatePrice("USDC", 1_000_000n, ts);

      expect(result.asset).toBe("USDC");
      expect(result.price).toBe(1_000_000n);
      expect(result.timestamp).toBe(ts);
    });

    it("succeeds for all five standard assets", async () => {
      const updater = makeUpdater();
      for (const asset of ["XLM", "BTC", "ETH", "USDC", "USDT"]) {
        const result = await updater.updatePrice(
          asset,
          100_000n,
          1_700_000_000,
        );
        expect(result.success).toBe(true);
        expect(result.asset).toBe(asset);
      }
    });

    it("single-character asset is accepted", async () => {
      const updater = makeUpdater();
      const result = await updater.updatePrice("X", 100n, 1_700_000_000);

      if (!result.success) {
        expect(result.error).not.toMatch(
          /non-empty string|maximum length|positive bigint|timestamp/i,
        );
      }
    });
  });

  // =========================================================================
  // healthCheck()
  // =========================================================================

  describe("healthCheck()", () => {
    it("returns true when Contract constructor succeeds", async () => {
      const updater = makeUpdater();
      const healthy = await updater.healthCheck();

      expect(healthy).toBe(true);
    });

    it("returns false when Contract constructor throws", async () => {
      const { Contract } = await import("@stellar/stellar-sdk");
      vi.mocked(Contract).mockImplementationOnce(() => {
        throw new Error("Invalid contract ID");
      });

      const updater = makeUpdater();
      const healthy = await updater.healthCheck();

      expect(healthy).toBe(false);
    });

    it("does not throw when Contract throws any error type", async () => {
      const { Contract } = await import("@stellar/stellar-sdk");
      vi.mocked(Contract).mockImplementationOnce(() => {
        throw new TypeError("Unexpected");
      });

      const updater = makeUpdater();
      await expect(updater.healthCheck()).resolves.toBe(false);
    });
  });

  // =========================================================================
  // Constructor & Configuration
  // =========================================================================

  describe("Constructor & Configuration", () => {
    it("selects testnet network passphrase without throwing", () => {
      expect(() => makeUpdater({ network: "testnet" })).not.toThrow();
    });

    it("selects mainnet network passphrase without throwing", () => {
      expect(() => makeUpdater({ network: "mainnet" })).not.toThrow();
    });

    it("getAdminPublicKey() returns the mocked public key", () => {
      const updater = makeUpdater();
      expect(updater.getAdminPublicKey()).toBe(
        "GADMIN_PUBLIC_KEY_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
      );
    });

    it("createContractUpdater() factory produces a ContractUpdater instance", () => {
      const updater = createContractUpdater(BASE_CONFIG);
      expect(updater).toBeInstanceOf(ContractUpdater);
    });

    it("applies DEFAULT_CONFIG when maxRetries/retryDelayMs are omitted", () => {
      const { maxRetries, retryDelayMs, ...minimal } = BASE_CONFIG;
      void maxRetries;
      void retryDelayMs;
      const updater = createContractUpdater(minimal as any);
      expect(updater).toBeInstanceOf(ContractUpdater);
    });

    it("respects custom txPollTimeoutMs – poll terminates quickly", async () => {
      mockServer.getTransaction.mockResolvedValue({ status: "NOT_FOUND" });

      const updater = makeUpdater({
        maxRetries: 1,
        txPollTimeoutMs: 20,
      });

      const result = await updater.updatePrice("XLM", 100_000n, 1_700_000_000);
      expect(result.success).toBe(false);
      expect(result.error).toMatch(/timed out/i);
    });

    it("MAX_ASSET_SYMBOL_BYTES is exported and equals 12", () => {
      expect(MAX_ASSET_SYMBOL_BYTES).toBe(12);
    });
  });

  // =========================================================================
  // Concurrency
  // =========================================================================

  describe("Concurrency", () => {
    it("handles concurrent updatePrice calls for the same asset", async () => {
      const updater = makeUpdater();
      const ts = 1_700_000_000;

      const promises = Array.from({ length: 10 }, (_, i) =>
        updater.updatePrice("XLM", BigInt(100_000 + i), ts),
      );
      const results = await Promise.all(promises);

      results.forEach((r, i) => {
        expect(r.success).toBe(true);
        expect(r.price).toBe(BigInt(100_000 + i));
      });
    });

    it("handles concurrent updatePrice calls for different assets", async () => {
      const updater = makeUpdater();
      const ts = 1_700_000_000;
      const assets = ["XLM", "BTC", "ETH", "USDC", "USDT"];

      const results = await Promise.all(
        assets.map((asset) => updater.updatePrice(asset, 100_000n, ts)),
      );

      results.forEach((r, i) => {
        expect(r.success).toBe(true);
        expect(r.asset).toBe(assets[i]);
      });
    });
  });

  // =========================================================================
  // Regression: Duplicate / Idempotent Submissions
  // =========================================================================

  describe("Regression: Duplicate / Idempotent Submissions", () => {
    it("submitting the same price twice does not throw", async () => {
      const updater = makeUpdater();
      const ts = 1_700_000_000;

      const r1 = await updater.updatePrice("XLM", 150_000n, ts);
      const r2 = await updater.updatePrice("XLM", 150_000n, ts);

      expect(r1.success).toBe(true);
      expect(r2.success).toBe(true);
    });

    it("duplicate assets in a batch are each submitted independently", async () => {
      const prices: AggregatedPrice[] = [
        makeAggregatedPrice("XLM", 150_000n, 1_700_000_000),
        makeAggregatedPrice("XLM", 150_001n, 1_700_000_001),
      ];

      const updater = makeUpdater();
      const results = await updater.updatePrices(prices);

      expect(results).toHaveLength(2);
      expect(results[0].success).toBe(true);
      expect(results[1].success).toBe(true);
    });
  });
});
