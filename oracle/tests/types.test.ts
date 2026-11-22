import { describe, it, expect } from "vitest";
import {
  ValidationErrorCode,
  PriceUpdateState,
  PriceUpdateStateTransitions,
  SUPPORTED_ASSETS,
  isSupportedAsset,
  isValidStateTransition,
  assertValidStateTransition,
  isTerminalState,
  isRetryableState,
  createPriceUpdateSession,
  transitionSession,
  canRetrySession,
  validateSessionInvariants,
  generateIdempotencyKey,
  validateIdempotencyKey,
  createSubmissionReceipt,
  isDuplicateSubmission,
  validateSequenceOrder,
  validatePriceData,
  validateRawPriceData,
  validateAggregatedPrice,
  validateFreshnessPolicy,
  validateFallbackPolicy,
  validateRecoveryPolicy,
  validateAuthorization,
  createValidationError,
  sanitizeSensitiveData,
  type PriceData,
  type RawPriceData,
  type AggregatedPrice,
  type AssetPriceBounds,
  type FreshnessPolicy,
  type FallbackPolicy,
  type RecoveryPolicy,
} from "../src/types/index.js";

describe("Oracle Types & Boundary Coverage", () => {
  describe("ValidationErrorCode Enum & Compatibility", () => {
    it("should define all required failure codes", () => {
      expect(ValidationErrorCode.PRICE_ZERO).toBe("PRICE_ZERO");
      expect(ValidationErrorCode.PRICE_NEGATIVE).toBe("PRICE_NEGATIVE");
      expect(ValidationErrorCode.PRICE_STALE).toBe("PRICE_STALE");
      expect(ValidationErrorCode.PRICE_DEVIATION_TOO_HIGH).toBe(
        "PRICE_DEVIATION_TOO_HIGH",
      );
      expect(ValidationErrorCode.PRICE_BELOW_MIN).toBe("PRICE_BELOW_MIN");
      expect(ValidationErrorCode.PRICE_ABOVE_MAX).toBe("PRICE_ABOVE_MAX");
      expect(ValidationErrorCode.INVALID_ASSET).toBe("INVALID_ASSET");
      expect(ValidationErrorCode.SOURCE_UNAVAILABLE).toBe("SOURCE_UNAVAILABLE");
      expect(ValidationErrorCode.DUPLICATE_SUBMISSION).toBe(
        "DUPLICATE_SUBMISSION",
      );
      expect(ValidationErrorCode.INVALID_STATE_TRANSITION).toBe(
        "INVALID_STATE_TRANSITION",
      );
      expect(ValidationErrorCode.RECOVERY_IN_PROGRESS).toBe(
        "RECOVERY_IN_PROGRESS",
      );
      expect(ValidationErrorCode.UNAUTHORIZED).toBe("UNAUTHORIZED");
      expect(ValidationErrorCode.INVALID_INPUT).toBe("INVALID_INPUT");
      expect(ValidationErrorCode.MAX_RETRIES_EXCEEDED).toBe(
        "MAX_RETRIES_EXCEEDED",
      );
      expect(ValidationErrorCode.SEQUENCE_REGRESSION).toBe(
        "SEQUENCE_REGRESSION",
      );
      expect(ValidationErrorCode.PARTIAL_FAILURE).toBe("PARTIAL_FAILURE");
    });

    it("should maintain backward compatibility for typo alias SOURCE_UNAVAILABL", () => {
      expect(ValidationErrorCode.SOURCE_UNAVAILABL).toBeDefined();
    });
  });

  describe("SupportedAsset & Helpers", () => {
    it("should correctly identify supported assets", () => {
      for (const asset of SUPPORTED_ASSETS) {
        expect(isSupportedAsset(asset)).toBe(true);
      }
    });

    it("should reject unsupported or invalid assets", () => {
      expect(isSupportedAsset("DOGE")).toBe(false);
      expect(isSupportedAsset("")).toBe(false);
      expect(isSupportedAsset(null)).toBe(false);
      expect(isSupportedAsset(undefined)).toBe(false);
      expect(isSupportedAsset(123)).toBe(false);
    });
  });

  describe("PriceUpdateState State Machine Transitions", () => {
    it("should validate all defined transitions from IDLE", () => {
      expect(
        isValidStateTransition(
          PriceUpdateState.IDLE,
          PriceUpdateState.FETCHING,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.IDLE,
          PriceUpdateState.CANCELLED,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(PriceUpdateState.IDLE, PriceUpdateState.SUCCESS),
      ).toBe(false);
      expect(
        isValidStateTransition(
          PriceUpdateState.IDLE,
          PriceUpdateState.SUBMITTING,
        ),
      ).toBe(false);
    });

    it("should validate forward pipeline progression", () => {
      expect(
        isValidStateTransition(
          PriceUpdateState.FETCHING,
          PriceUpdateState.VALIDATING,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.VALIDATING,
          PriceUpdateState.AGGREGATING,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.AGGREGATING,
          PriceUpdateState.SUBMITTING,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.SUBMITTING,
          PriceUpdateState.SUCCESS,
        ),
      ).toBe(true);
    });

    it("should allow failure and cancellation from intermediate states", () => {
      expect(
        isValidStateTransition(
          PriceUpdateState.FETCHING,
          PriceUpdateState.FAILED,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.VALIDATING,
          PriceUpdateState.FAILED,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.AGGREGATING,
          PriceUpdateState.FAILED,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.SUBMITTING,
          PriceUpdateState.FAILED,
        ),
      ).toBe(true);

      expect(
        isValidStateTransition(
          PriceUpdateState.FETCHING,
          PriceUpdateState.CANCELLED,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.VALIDATING,
          PriceUpdateState.CANCELLED,
        ),
      ).toBe(true);
    });

    it("should validate recovery and retry transitions", () => {
      expect(
        isValidStateTransition(
          PriceUpdateState.FAILED,
          PriceUpdateState.RETRYING,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.FAILED,
          PriceUpdateState.RECOVERING,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.RETRYING,
          PriceUpdateState.FETCHING,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.RETRYING,
          PriceUpdateState.SUBMITTING,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.RECOVERING,
          PriceUpdateState.FETCHING,
        ),
      ).toBe(true);
      expect(
        isValidStateTransition(
          PriceUpdateState.RECOVERING,
          PriceUpdateState.SUBMITTING,
        ),
      ).toBe(true);
    });

    it("should correctly enforce terminal states with no exit transitions", () => {
      expect(isTerminalState(PriceUpdateState.SUCCESS)).toBe(true);
      expect(isTerminalState(PriceUpdateState.CANCELLED)).toBe(true);
      expect(isTerminalState(PriceUpdateState.IDLE)).toBe(false);
      expect(isTerminalState(PriceUpdateState.FAILED)).toBe(false);

      expect(
        isValidStateTransition(PriceUpdateState.SUCCESS, PriceUpdateState.IDLE),
      ).toBe(false);
      expect(
        isValidStateTransition(
          PriceUpdateState.CANCELLED,
          PriceUpdateState.FETCHING,
        ),
      ).toBe(false);
    });

    it("should identify retryable states", () => {
      expect(isRetryableState(PriceUpdateState.FAILED)).toBe(true);
      expect(isRetryableState(PriceUpdateState.RETRYING)).toBe(true);
      expect(isRetryableState(PriceUpdateState.RECOVERING)).toBe(true);
      expect(isRetryableState(PriceUpdateState.IDLE)).toBe(false);
      expect(isRetryableState(PriceUpdateState.SUCCESS)).toBe(false);
    });

    it("should throw on invalid transition with assertValidStateTransition", () => {
      expect(() =>
        assertValidStateTransition(
          PriceUpdateState.IDLE,
          PriceUpdateState.FETCHING,
        ),
      ).not.toThrow();
      expect(() =>
        assertValidStateTransition(
          PriceUpdateState.IDLE,
          PriceUpdateState.SUCCESS,
        ),
      ).toThrowError(
        expect.objectContaining({
          code: ValidationErrorCode.INVALID_STATE_TRANSITION,
        }),
      );
    });

    it("should handle invalid/corrupt state input gracefully", () => {
      expect(
        isValidStateTransition("INVALID_STATE", PriceUpdateState.IDLE),
      ).toBe(false);
      expect(isValidStateTransition(PriceUpdateState.IDLE, null)).toBe(false);
      expect(isValidStateTransition(undefined, undefined)).toBe(false);
      expect(isValidStateTransition(123 as any, PriceUpdateState.IDLE)).toBe(
        false,
      );
    });
  });

  describe("PriceUpdateSession Management & Retries", () => {
    it("should create an immutable session with default options", () => {
      const session = createPriceUpdateSession({
        sessionId: "session-1",
        asset: "XLM",
      });

      expect(session.sessionId).toBe("session-1");
      expect(session.asset).toBe("XLM");
      expect(session.state).toBe(PriceUpdateState.IDLE);
      expect(session.attemptCount).toBe(0);
      expect(session.maxAttempts).toBe(3);
      expect(session.idempotencyKey).toBeDefined();
      expect(Object.isFrozen(session)).toBe(true);
    });

    it("should accept custom session options", () => {
      const session = createPriceUpdateSession({
        sessionId: "session-custom",
        asset: "BTC",
        maxAttempts: 5,
        idempotencyKey: "custom-key",
        userIntent: "rebalance",
        requestedAt: 1700000000,
      });

      expect(session.sessionId).toBe("session-custom");
      expect(session.asset).toBe("BTC");
      expect(session.maxAttempts).toBe(5);
      expect(session.idempotencyKey).toBe("custom-key");
      expect(session.userIntent).toBe("rebalance");
      expect(session.requestedAt).toBe(1700000000);
    });

    it("should reject session creation with invalid boundaries", () => {
      expect(() => createPriceUpdateSession(null as any)).toThrow();
      expect(() =>
        createPriceUpdateSession({ sessionId: "", asset: "XLM" }),
      ).toThrow();
      expect(() =>
        createPriceUpdateSession({ sessionId: "  ", asset: "XLM" }),
      ).toThrow();
      expect(() =>
        createPriceUpdateSession({ sessionId: "s1", asset: "INVALID" as any }),
      ).toThrow();
      expect(() =>
        createPriceUpdateSession({
          sessionId: "s1",
          asset: "XLM",
          maxAttempts: 0,
        }),
      ).toThrow();
      expect(() =>
        createPriceUpdateSession({
          sessionId: "s1",
          asset: "XLM",
          maxAttempts: -1,
        }),
      ).toThrow();
      expect(() =>
        createPriceUpdateSession({
          sessionId: "s1",
          asset: "XLM",
          maxAttempts: 1.5,
        }),
      ).toThrow();
      expect(() =>
        createPriceUpdateSession({
          sessionId: "s1",
          asset: "XLM",
          maxAttempts: "three" as any,
        }),
      ).toThrow();
    });

    it("should advance session state immutably without mutating previous state", () => {
      const s0 = createPriceUpdateSession({ sessionId: "s1", asset: "XLM" });
      const s1 = transitionSession(s0, PriceUpdateState.FETCHING, {
        transactionHash: "tx-hash-1",
        recoveryState: { checkpoint: 1 },
      });

      expect(s0.state).toBe(PriceUpdateState.IDLE);
      expect(s1.state).toBe(PriceUpdateState.FETCHING);
      expect(s1.attemptCount).toBe(0);
      expect(s1.transactionHash).toBe("tx-hash-1");
      expect(s1.recoveryState).toEqual({ checkpoint: 1 });
      expect(Object.isFrozen(s1)).toBe(true);
    });

    it("should increment attempt count and enforce maxAttempts boundary on RETRYING", () => {
      let session = createPriceUpdateSession({
        sessionId: "s1",
        asset: "XLM",
        maxAttempts: 2,
      });
      session = transitionSession(session, PriceUpdateState.FETCHING);
      session = transitionSession(session, PriceUpdateState.FAILED);

      expect(canRetrySession(session)).toBe(true);

      // Attempt 1
      session = transitionSession(session, PriceUpdateState.RETRYING);
      expect(session.attemptCount).toBe(1);

      session = transitionSession(session, PriceUpdateState.FAILED);
      expect(canRetrySession(session)).toBe(true);

      // Attempt 2
      session = transitionSession(session, PriceUpdateState.RETRYING);
      expect(session.attemptCount).toBe(2);

      session = transitionSession(session, PriceUpdateState.FAILED);
      expect(canRetrySession(session)).toBe(false);

      // Exceeds maxAttempts (attempt 3 when max is 2)
      expect(() =>
        transitionSession(session, PriceUpdateState.RETRYING),
      ).toThrowError(
        expect.objectContaining({
          code: ValidationErrorCode.MAX_RETRIES_EXCEEDED,
        }),
      );
    });

    it("should reject invalid transition input in transitionSession", () => {
      expect(() =>
        transitionSession(null as any, PriceUpdateState.FETCHING),
      ).toThrow();
      const session = createPriceUpdateSession({
        sessionId: "s1",
        asset: "XLM",
      });
      expect(() =>
        transitionSession(session, PriceUpdateState.SUCCESS),
      ).toThrow();
    });

    it("should safely handle canRetrySession with null/undefined", () => {
      expect(canRetrySession(null as any)).toBe(false);
      expect(canRetrySession(undefined as any)).toBe(false);
    });

    it("should validate session invariants", () => {
      const valid = createPriceUpdateSession({ sessionId: "s1", asset: "BTC" });
      expect(validateSessionInvariants(valid).isValid).toBe(true);

      expect(validateSessionInvariants(null).isValid).toBe(false);
      expect(
        validateSessionInvariants({ ...valid, sessionId: "" }).isValid,
      ).toBe(false);
      expect(
        validateSessionInvariants({ ...valid, state: "NOT_A_STATE" as any })
          .isValid,
      ).toBe(false);
      expect(
        validateSessionInvariants({ ...valid, maxAttempts: 0 }).isValid,
      ).toBe(false);
      expect(
        validateSessionInvariants({ ...valid, attemptCount: -1 }).isValid,
      ).toBe(false);
      expect(
        validateSessionInvariants({ ...valid, attemptCount: 5, maxAttempts: 3 })
          .isValid,
      ).toBe(false);
      expect(
        validateSessionInvariants({
          ...valid,
          updatedAt: valid.createdAt - 1000,
        }).isValid,
      ).toBe(false);
      expect(
        validateSessionInvariants({ ...valid, asset: "DOGE" as any }).isValid,
      ).toBe(false);
    });
  });

  describe("Idempotency & Duplicate Submission Boundaries", () => {
    it("should generate deterministic idempotency keys", () => {
      const key1 = generateIdempotencyKey("XLM", 1700000000, 1);
      const key2 = generateIdempotencyKey("XLM", 1700000000, 1);
      const key3 = generateIdempotencyKey("XLM", 1700000000, 2);

      expect(key1).toBe("XLM:1700000000:1");
      expect(key1).toBe(key2);
      expect(key1).not.toBe(key3);
    });

    it("should reject invalid inputs for generateIdempotencyKey", () => {
      expect(() => generateIdempotencyKey("INVALID" as any, 1000)).toThrow();
      expect(() => generateIdempotencyKey("XLM", -1)).toThrow();
      expect(() => generateIdempotencyKey("XLM", 0)).toThrow();
      expect(() => generateIdempotencyKey("XLM", NaN)).toThrow();
      expect(() => generateIdempotencyKey("XLM", Infinity)).toThrow();
    });

    it("should validate idempotency key constraints", () => {
      expect(validateIdempotencyKey("valid-key-123")).toBe(true);
      expect(validateIdempotencyKey("")).toBe(false);
      expect(validateIdempotencyKey("   ")).toBe(false);
      expect(validateIdempotencyKey(null)).toBe(false);
      expect(validateIdempotencyKey(123)).toBe(false);
      expect(validateIdempotencyKey("a".repeat(257))).toBe(false);
      expect(validateIdempotencyKey("a".repeat(256))).toBe(true);
    });

    it("should create submission receipt with boundary checks", () => {
      const receipt = createSubmissionReceipt({
        idempotencyKey: "key-1",
        asset: "ETH",
        price: 3000000000n,
        timestamp: 1700000000,
        success: true,
        attempt: 1,
      });

      expect(receipt.idempotencyKey).toBe("key-1");
      expect(receipt.price).toBe(3000000000n);
      expect(Object.isFrozen(receipt)).toBe(true);

      expect(() =>
        createSubmissionReceipt({
          idempotencyKey: "",
          asset: "ETH",
          price: 3000n,
          timestamp: 1700000000,
          success: true,
          attempt: 1,
        }),
      ).toThrow();

      expect(() =>
        createSubmissionReceipt({
          idempotencyKey: "key-1",
          asset: "DOGE" as any,
          price: 3000n,
          timestamp: 1700000000,
          success: true,
          attempt: 1,
        }),
      ).toThrow();

      expect(() =>
        createSubmissionReceipt({
          idempotencyKey: "key-1",
          asset: "ETH",
          price: 0n,
          timestamp: 1700000000,
          success: true,
          attempt: 1,
        }),
      ).toThrow();

      expect(() =>
        createSubmissionReceipt({
          idempotencyKey: "key-1",
          asset: "ETH",
          price: 100n,
          timestamp: -1,
          success: true,
          attempt: 1,
        }),
      ).toThrow();

      expect(() =>
        createSubmissionReceipt({
          idempotencyKey: "key-1",
          asset: "ETH",
          price: 100n,
          timestamp: 1700000000,
          success: true,
          attempt: 0,
        }),
      ).toThrow();
    });

    it("should correctly detect duplicate submissions", () => {
      const receipt = createSubmissionReceipt({
        idempotencyKey: "unique-key-1",
        asset: "USDC",
        price: 1000000n,
        timestamp: 1700000000,
        success: true,
        attempt: 1,
      });

      // Same idempotency key
      expect(
        isDuplicateSubmission(receipt, {
          idempotencyKey: "unique-key-1",
          asset: "USDC",
          timestamp: 1700000000,
        }),
      ).toBe(true);

      // Same asset & timestamp
      expect(
        isDuplicateSubmission(receipt, {
          idempotencyKey: "different-key",
          asset: "USDC",
          timestamp: 1700000000,
        }),
      ).toBe(true);

      // Different asset & timestamp
      expect(
        isDuplicateSubmission(receipt, {
          idempotencyKey: "unique-key-2",
          asset: "BTC",
          timestamp: 1700000001,
        }),
      ).toBe(false);

      // Null handling
      expect(isDuplicateSubmission(null as any, null as any)).toBe(false);
    });
  });

  describe("Sequence Monotonicity & Ordering", () => {
    it("should accept strictly increasing or equal sequences", () => {
      expect(validateSequenceOrder(1, 2).isValid).toBe(true);
      expect(validateSequenceOrder(5, 5).isValid).toBe(true);
      expect(validateSequenceOrder(undefined, 1).isValid).toBe(true);
      expect(validateSequenceOrder(1, undefined).isValid).toBe(true);
      expect(validateSequenceOrder(undefined, undefined).isValid).toBe(true);
    });

    it("should reject sequence regression (stale/out-of-order sequence)", () => {
      const res = validateSequenceOrder(10, 9);
      expect(res.isValid).toBe(false);
      expect(res.errors[0].code).toBe(ValidationErrorCode.SEQUENCE_REGRESSION);
    });

    it("should reject invalid sequence number values", () => {
      expect(validateSequenceOrder(1, -1).isValid).toBe(false);
      expect(validateSequenceOrder(1, 1.5).isValid).toBe(false);
      expect(validateSequenceOrder(1, "10" as any).isValid).toBe(false);
    });
  });

  describe("PriceData Boundary Validation", () => {
    const validPrice: PriceData = {
      asset: "XLM",
      price: 150000n,
      timestamp: Math.floor(Date.now() / 1000),
      source: "binance",
      confidence: 95,
    };

    it("should reject null or non-object price data", () => {
      expect(validatePriceData(null).isValid).toBe(false);
      expect(validatePriceData(undefined).isValid).toBe(false);
      expect(validatePriceData("string").isValid).toBe(false);
    });

    it("should accept valid PriceData for all supported assets", () => {
      for (const asset of SUPPORTED_ASSETS) {
        const res = validatePriceData({ ...validPrice, asset });
        expect(res.isValid).toBe(true);
        expect(res.price).toBeDefined();
      }
    });

    it("should reject invalid or unsupported asset", () => {
      const res = validatePriceData({ ...validPrice, asset: "DOGE" });
      expect(res.isValid).toBe(false);
      expect(res.errors[0].code).toBe(ValidationErrorCode.INVALID_ASSET);
    });

    it("should reject price that is not bigint", () => {
      const res = validatePriceData({ ...validPrice, price: 150000 as any });
      expect(res.isValid).toBe(false);
      expect(res.errors[0].code).toBe(ValidationErrorCode.INVALID_INPUT);
    });

    it("should reject price equal to zero", () => {
      const res = validatePriceData({ ...validPrice, price: 0n });
      expect(res.isValid).toBe(false);
      expect(res.errors[0].code).toBe(ValidationErrorCode.PRICE_ZERO);
    });

    it("should reject negative price", () => {
      const res = validatePriceData({ ...validPrice, price: -100n });
      expect(res.isValid).toBe(false);
      expect(res.errors[0].code).toBe(ValidationErrorCode.PRICE_NEGATIVE);
    });

    it("should accept minimum positive boundary price of 1n", () => {
      const res = validatePriceData({ ...validPrice, price: 1n });
      expect(res.isValid).toBe(true);
    });

    it("should enforce AssetPriceBounds boundaries", () => {
      const bounds: AssetPriceBounds = { minPrice: 0.1, maxPrice: 10.0 }; // scaled 100,000 to 10,000,000

      // Within bounds
      expect(
        validatePriceData({ ...validPrice, price: 500000n }, bounds).isValid,
      ).toBe(true);

      // Exact min boundary
      expect(
        validatePriceData({ ...validPrice, price: 100000n }, bounds).isValid,
      ).toBe(true);

      // Exact max boundary
      expect(
        validatePriceData({ ...validPrice, price: 10000000n }, bounds).isValid,
      ).toBe(true);

      // Below min
      const below = validatePriceData({ ...validPrice, price: 99999n }, bounds);
      expect(below.isValid).toBe(false);
      expect(below.errors[0].code).toBe(ValidationErrorCode.PRICE_BELOW_MIN);

      // Above max
      const above = validatePriceData(
        { ...validPrice, price: 10000001n },
        bounds,
      );
      expect(above.isValid).toBe(false);
      expect(above.errors[0].code).toBe(ValidationErrorCode.PRICE_ABOVE_MAX);
    });

    it("should validate confidence score boundaries [0, 100]", () => {
      expect(validatePriceData({ ...validPrice, confidence: 0 }).isValid).toBe(
        true,
      );
      expect(
        validatePriceData({ ...validPrice, confidence: 100 }).isValid,
      ).toBe(true);
      expect(validatePriceData({ ...validPrice, confidence: -1 }).isValid).toBe(
        false,
      );
      expect(
        validatePriceData({ ...validPrice, confidence: 101 }).isValid,
      ).toBe(false);
      expect(
        validatePriceData({ ...validPrice, confidence: NaN }).isValid,
      ).toBe(false);
    });

    it("should validate timestamp boundaries and reject timestamps too far in the future", () => {
      const nowSec = Math.floor(Date.now() / 1000);
      expect(
        validatePriceData({ ...validPrice, timestamp: nowSec }).isValid,
      ).toBe(true);
      expect(validatePriceData({ ...validPrice, timestamp: 0 }).isValid).toBe(
        false,
      );
      expect(validatePriceData({ ...validPrice, timestamp: -1 }).isValid).toBe(
        false,
      );

      // 1000 seconds into future exceeds standard 60s skew tolerance
      const resFuture = validatePriceData({
        ...validPrice,
        timestamp: nowSec + 1000,
      });
      expect(resFuture.isValid).toBe(false);
      expect(resFuture.errors[0].code).toBe(ValidationErrorCode.PRICE_STALE);
    });

    it("should validate source non-empty string", () => {
      expect(validatePriceData({ ...validPrice, source: "" }).isValid).toBe(
        false,
      );
      expect(validatePriceData({ ...validPrice, source: "   " }).isValid).toBe(
        false,
      );
      expect(
        validatePriceData({ ...validPrice, source: null as any }).isValid,
      ).toBe(false);
    });

    it("should validate volume24h boundaries", () => {
      expect(validatePriceData({ ...validPrice, volume24h: 0n }).isValid).toBe(
        true,
      );
      expect(
        validatePriceData({ ...validPrice, volume24h: 10000000n }).isValid,
      ).toBe(true);
      expect(validatePriceData({ ...validPrice, volume24h: -1n }).isValid).toBe(
        false,
      );
      expect(
        validatePriceData({ ...validPrice, volume24h: 1000 as any }).isValid,
      ).toBe(false);
    });

    it("should require both signer and signature when signature verification is present", () => {
      expect(
        validatePriceData({
          ...validPrice,
          signer: "GAA...",
          signature: "SIG...",
        }).isValid,
      ).toBe(true);

      expect(
        validatePriceData({
          ...validPrice,
          signer: "GAA...",
        }).isValid,
      ).toBe(false);

      expect(
        validatePriceData({
          ...validPrice,
          signature: "SIG...",
        }).isValid,
      ).toBe(false);
    });
  });

  describe("RawPriceData Boundary Validation", () => {
    const validRaw: RawPriceData = {
      asset: "XLM",
      price: 0.15,
      timestamp: Math.floor(Date.now() / 1000),
      source: "binance",
    };

    it("should reject null or non-object raw price", () => {
      expect(validateRawPriceData(null).isValid).toBe(false);
    });

    it("should accept valid raw prices", () => {
      expect(validateRawPriceData(validRaw).isValid).toBe(true);
    });

    it("should reject invalid asset in raw prices", () => {
      expect(
        validateRawPriceData({ ...validRaw, asset: "UNKNOWN" as any }).isValid,
      ).toBe(false);
    });

    it("should reject zero, negative, NaN, and Infinity", () => {
      expect(validateRawPriceData({ ...validRaw, price: 0 }).isValid).toBe(
        false,
      );
      expect(validateRawPriceData({ ...validRaw, price: -0.15 }).isValid).toBe(
        false,
      );
      expect(validateRawPriceData({ ...validRaw, price: NaN }).isValid).toBe(
        false,
      );
      expect(
        validateRawPriceData({ ...validRaw, price: Infinity }).isValid,
      ).toBe(false);
      expect(
        validateRawPriceData({ ...validRaw, price: -Infinity }).isValid,
      ).toBe(false);
      expect(
        validateRawPriceData({ ...validRaw, price: "0.15" as any }).isValid,
      ).toBe(false);
    });

    it("should reject invalid timestamp or empty source in raw prices", () => {
      expect(validateRawPriceData({ ...validRaw, timestamp: 0 }).isValid).toBe(
        false,
      );
      expect(validateRawPriceData({ ...validRaw, timestamp: -5 }).isValid).toBe(
        false,
      );
      expect(validateRawPriceData({ ...validRaw, source: "" }).isValid).toBe(
        false,
      );
    });

    it("should enforce AssetPriceBounds on raw prices", () => {
      const bounds: AssetPriceBounds = { minPrice: 0.1, maxPrice: 1.0 };
      expect(
        validateRawPriceData({ ...validRaw, price: 0.5 }, bounds).isValid,
      ).toBe(true);
      expect(
        validateRawPriceData({ ...validRaw, price: 0.05 }, bounds).isValid,
      ).toBe(false);
      expect(
        validateRawPriceData({ ...validRaw, price: 1.5 }, bounds).isValid,
      ).toBe(false);
    });
  });

  describe("AggregatedPrice Boundary Validation", () => {
    const sourceData: PriceData = {
      asset: "USDC",
      price: 1000000n,
      timestamp: Math.floor(Date.now() / 1000),
      source: "binance",
      confidence: 90,
    };

    it("should reject null or non-object aggregated price", () => {
      expect(validateAggregatedPrice(null).isValid).toBe(false);
    });

    it("should validate a complete aggregated price", () => {
      const agg: AggregatedPrice = {
        asset: "USDC",
        price: 1000000n,
        sources: [sourceData],
        timestamp: Math.floor(Date.now() / 1000),
        confidence: 90,
        sequence: 1,
      };

      expect(validateAggregatedPrice(agg).isValid).toBe(true);
    });

    it("should reject empty or invalid sources list", () => {
      const res = validateAggregatedPrice({
        asset: "USDC",
        price: 1000000n,
        sources: [],
        timestamp: Math.floor(Date.now() / 1000),
        confidence: 90,
      });
      expect(res.isValid).toBe(false);

      const resInvalid = validateAggregatedPrice({
        asset: "USDC",
        price: 1000000n,
        sources: [{ ...sourceData, price: 0n }],
        timestamp: Math.floor(Date.now() / 1000),
        confidence: 90,
      });
      expect(resInvalid.isValid).toBe(false);
    });

    it("should reject invalid asset or non-positive price", () => {
      expect(
        validateAggregatedPrice({
          asset: "BAD_ASSET" as any,
          price: 1000000n,
          sources: [sourceData],
          confidence: 90,
        }).isValid,
      ).toBe(false);

      expect(
        validateAggregatedPrice({
          asset: "USDC",
          price: 0n,
          sources: [sourceData],
          confidence: 90,
        }).isValid,
      ).toBe(false);
    });

    it("should reject invalid confidence or sequence", () => {
      expect(
        validateAggregatedPrice({
          asset: "USDC",
          price: 1000000n,
          sources: [sourceData],
          timestamp: Math.floor(Date.now() / 1000),
          confidence: 101,
        }).isValid,
      ).toBe(false);

      expect(
        validateAggregatedPrice({
          asset: "USDC",
          price: 1000000n,
          sources: [sourceData],
          timestamp: Math.floor(Date.now() / 1000),
          confidence: 90,
          sequence: -5,
        }).isValid,
      ).toBe(false);
    });
  });

  describe("Policy Validation Boundaries", () => {
    describe("FreshnessPolicy", () => {
      const validFresh: FreshnessPolicy = {
        maxStalenessSeconds: 300,
        maxDeviationPercent: 10,
        requireFresh: true,
        fallbackOnStale: false,
      };

      it("should reject null or non-object policy", () => {
        expect(validateFreshnessPolicy(null).isValid).toBe(false);
      });

      it("should accept valid policy", () => {
        expect(validateFreshnessPolicy(validFresh).isValid).toBe(true);
      });

      it("should reject non-positive staleness or out-of-range deviation", () => {
        expect(
          validateFreshnessPolicy({ ...validFresh, maxStalenessSeconds: 0 })
            .isValid,
        ).toBe(false);
        expect(
          validateFreshnessPolicy({ ...validFresh, maxStalenessSeconds: -1 })
            .isValid,
        ).toBe(false);
        expect(
          validateFreshnessPolicy({ ...validFresh, maxDeviationPercent: -1 })
            .isValid,
        ).toBe(false);
        expect(
          validateFreshnessPolicy({ ...validFresh, maxDeviationPercent: 101 })
            .isValid,
        ).toBe(false);
        expect(
          validateFreshnessPolicy({ ...validFresh, requireFresh: "yes" as any })
            .isValid,
        ).toBe(false);
        expect(
          validateFreshnessPolicy({
            ...validFresh,
            fallbackOnStale: "no" as any,
          }).isValid,
        ).toBe(false);
      });
    });

    describe("FallbackPolicy", () => {
      const validFallback: FallbackPolicy = {
        enabled: true,
        fallbackOrder: "priority",
        preferHighestConfidence: true,
        minSources: 2,
        useVolumeWeightedMedian: true,
        maxFallbackAttempts: 3,
      };

      it("should reject null or non-object policy", () => {
        expect(validateFallbackPolicy(null).isValid).toBe(false);
      });

      it("should accept valid policy", () => {
        expect(validateFallbackPolicy(validFallback).isValid).toBe(true);
      });

      it("should reject invalid fallbackOrder, negative attempts, or minSources < 1", () => {
        expect(
          validateFallbackPolicy({ ...validFallback, enabled: "true" as any })
            .isValid,
        ).toBe(false);
        expect(
          validateFallbackPolicy({ ...validFallback, minSources: 0 }).isValid,
        ).toBe(false);
        expect(
          validateFallbackPolicy({ ...validFallback, maxFallbackAttempts: -1 })
            .isValid,
        ).toBe(false);
        expect(
          validateFallbackPolicy({
            ...validFallback,
            fallbackOrder: "random" as any,
          }).isValid,
        ).toBe(false);
      });
    });

    describe("RecoveryPolicy", () => {
      const validRecovery: RecoveryPolicy = {
        enabled: true,
        preserveUserIntent: true,
        idempotentRetries: true,
        resumeFromPersistedState: true,
        statePersistence: "memory",
        timeoutSeconds: 60,
      };

      it("should reject null or non-object policy", () => {
        expect(validateRecoveryPolicy(null).isValid).toBe(false);
      });

      it("should accept valid policy", () => {
        expect(validateRecoveryPolicy(validRecovery).isValid).toBe(true);
      });

      it("should reject non-positive timeout or unknown persistence store", () => {
        expect(
          validateRecoveryPolicy({ ...validRecovery, enabled: 123 as any })
            .isValid,
        ).toBe(false);
        expect(
          validateRecoveryPolicy({ ...validRecovery, timeoutSeconds: 0 })
            .isValid,
        ).toBe(false);
        expect(
          validateRecoveryPolicy({ ...validRecovery, timeoutSeconds: -10 })
            .isValid,
        ).toBe(false);
        expect(
          validateRecoveryPolicy({
            ...validRecovery,
            statePersistence: "mysql" as any,
          }).isValid,
        ).toBe(false);
      });
    });
  });

  describe("Observability & Sensitive Data Redaction", () => {
    it("should redact sensitive keys and Stellar secret keys from details", () => {
      const secretKey = "S" + "A".repeat(55);
      const rawDetails = {
        asset: "XLM",
        apiKey: "super-secret-api-key",
        adminSecretKey: secretKey,
        nested: {
          token: "bearer-token",
          attempt: 2,
          volume: 1000000n,
          list: ["safe-entry", secretKey],
        },
      };

      const sanitized = sanitizeSensitiveData(rawDetails) as Record<
        string,
        any
      >;

      expect(sanitized.asset).toBe("XLM");
      expect(sanitized.apiKey).toBe("[REDACTED]");
      expect(sanitized.adminSecretKey).toBe("[REDACTED]");
      expect(sanitized.nested.token).toBe("[REDACTED]");
      expect(sanitized.nested.attempt).toBe(2);
      expect(sanitized.nested.volume).toBe("1000000");
      expect(sanitized.nested.list[1]).toContain("S***[REDACTED]");
    });

    it("should handle primitives and recursion limits safely in sanitizeSensitiveData", () => {
      expect(sanitizeSensitiveData(null)).toBe(null);
      expect(sanitizeSensitiveData(undefined)).toBe(undefined);
      expect(sanitizeSensitiveData(42)).toBe(42);
      expect(sanitizeSensitiveData("hello", 10)).toBe("hello");
    });

    it("should redact secret keys embedded in error message strings", () => {
      const secretKey = "S" + "B".repeat(55);
      const err = createValidationError(
        ValidationErrorCode.UNAUTHORIZED,
        `Failed auth with key ${secretKey}`,
        { key: secretKey },
      );

      expect(err.message).toContain("S***[REDACTED]");
      expect(err.message).not.toContain(secretKey);
      expect(err.details?.key).toBe("[REDACTED]");
    });

    it("should provide default message if non-string is passed to createValidationError", () => {
      const err = createValidationError(
        ValidationErrorCode.INVALID_INPUT,
        null as any,
      );
      expect(err.message).toBe("Validation error");
    });
  });

  describe("Authorization Boundary Validation", () => {
    it("should permit access when API keys match", () => {
      const res = validateAuthorization({
        apiKey: "valid-api-key",
        expectedApiKey: "valid-api-key",
      });
      expect(res.isValid).toBe(true);
    });

    it("should permit access when no expected key is configured", () => {
      const res = validateAuthorization({});
      expect(res.isValid).toBe(true);
    });

    it("should reject access with UNAUTHORIZED when key does not match without leaking expected key", () => {
      const res = validateAuthorization({
        apiKey: "attacker-key",
        expectedApiKey: "secret-system-key",
      });
      expect(res.isValid).toBe(false);
      expect(res.errors[0].code).toBe(ValidationErrorCode.UNAUTHORIZED);
      expect(res.errors[0].message).not.toContain("secret-system-key");
    });

    it("should reject access when apiKey is missing but expectedApiKey is set", () => {
      const res = validateAuthorization({
        expectedApiKey: "secret-system-key",
      });
      expect(res.isValid).toBe(false);
      expect(res.errors[0].code).toBe(ValidationErrorCode.UNAUTHORIZED);
    });
  });
});
