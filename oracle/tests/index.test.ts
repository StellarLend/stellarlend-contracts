import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  OracleService,
  createOracleService,
  validateOracleConfig,
  sanitizeLogConfig,
  DEFAULT_ASSETS,
} from "../src/index.js";
import type {
  OracleServiceConfig,
  ProviderConfig,
  AggregatedPrice,
  ContractUpdateResult,
} from "../src/types/index.js";

describe("Oracle Service (oracle/src/index.ts) Boundary & Failure Path Coverage", () => {
  let mockConfig: OracleServiceConfig;
  let mockAggregator: any;
  let mockUpdater: any;
  let mockAdminServer: any;

  beforeEach(() => {
    mockConfig = {
      stellarNetwork: "testnet",
      stellarRpcUrl: "https://soroban-testnet.stellar.org",
      contractId: "CCONTRACT123",
      adminSecretKey: "S" + "A".repeat(55),
      adminApiPort: 8080,
      adminHmacSecret: "hmac-secret-test",
      updateIntervalMs: 1000,
      maxPriceDeviationPercent: 10,
      madZScoreThreshold: 3.5,
      priceStaleThresholdSeconds: 300,
      cacheTtlSeconds: 30,
      logLevel: "info",
      providers: [
        {
          name: "coingecko",
          enabled: true,
          priority: 1,
          weight: 0.6,
          baseUrl: "https://api.coingecko.com",
          rateLimit: { maxRequests: 10, windowMs: 60000 },
        },
        {
          name: "binance",
          enabled: true,
          priority: 2,
          weight: 0.4,
          baseUrl: "https://api.binance.com",
          rateLimit: { maxRequests: 1200, windowMs: 60000 },
        },
      ],
    };

    mockAggregator = {
      getPrices: vi.fn().mockResolvedValue(
        new Map<string, AggregatedPrice>([
          [
            "XLM",
            {
              asset: "XLM",
              price: 150000n,
              timestamp: 1700000000,
              confidence: 95,
              sources: [],
            },
          ],
        ]),
      ),
      getPrice: vi.fn().mockResolvedValue({
        asset: "XLM",
        price: 150000n,
        timestamp: 1700000000,
        confidence: 95,
        sources: [],
      }),
      getProviders: vi.fn().mockReturnValue(["coingecko", "binance"]),
      getStats: vi.fn().mockReturnValue({ cacheHits: 5, cacheMisses: 2 }),
    };

    mockUpdater = {
      updatePrices: vi.fn().mockResolvedValue([
        {
          success: true,
          asset: "XLM",
          price: 150000n,
          timestamp: 1700000000,
        },
      ]),
    };

    mockAdminServer = {
      start: vi.fn().mockResolvedValue(undefined),
      stop: vi.fn().mockResolvedValue(undefined),
    };
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("validateOracleConfig Boundaries", () => {
    it("should pass on valid configuration", () => {
      expect(() => validateOracleConfig(mockConfig)).not.toThrow();
    });

    it("should throw when config is null or non-object", () => {
      expect(() => validateOracleConfig(null as any)).toThrow(
        "Oracle configuration must be a non-null object",
      );
      expect(() => validateOracleConfig(undefined as any)).toThrow();
      expect(() => validateOracleConfig("config" as any)).toThrow();
    });

    it("should throw on invalid stellarNetwork", () => {
      expect(() =>
        validateOracleConfig({
          ...mockConfig,
          stellarNetwork: "localnet" as any,
        }),
      ).toThrow(/Invalid stellarNetwork/);
    });

    it("should throw on invalid stellarRpcUrl", () => {
      expect(() =>
        validateOracleConfig({ ...mockConfig, stellarRpcUrl: "invalid-url" }),
      ).toThrow(/stellarRpcUrl must be a valid HTTP\/HTTPS URL/);
      expect(() =>
        validateOracleConfig({ ...mockConfig, stellarRpcUrl: "" }),
      ).toThrow();
    });

    it("should throw when contractId is empty string or whitespace", () => {
      expect(() =>
        validateOracleConfig({ ...mockConfig, contractId: "" }),
      ).toThrow(/contractId is required/);
      expect(() =>
        validateOracleConfig({ ...mockConfig, contractId: "   " }),
      ).toThrow(/contractId is required/);
    });

    it("should throw when adminApiPort > 0 but adminHmacSecret is missing or empty", () => {
      expect(() =>
        validateOracleConfig({
          ...mockConfig,
          adminApiPort: 8080,
          adminHmacSecret: "",
        }),
      ).toThrow(/ADMIN_HMAC_SECRET is required/);
      expect(() =>
        validateOracleConfig({
          ...mockConfig,
          adminApiPort: 8080,
          adminHmacSecret: undefined,
        }),
      ).toThrow(/ADMIN_HMAC_SECRET is required/);
    });
  });

  describe("sanitizeLogConfig (Sensitive Data Redaction)", () => {
    it("should redact secret keys, HMAC, passwords, and tokens", () => {
      const rawConfig = {
        stellarNetwork: "testnet",
        contractId: "C123",
        adminSecretKey: "SABC123",
        adminHmacSecret: "hmac-super-secret",
        apiKey: "api-key-123",
        password: "my-password",
        token: "bearer-token",
      };

      const sanitized = sanitizeLogConfig(rawConfig as any);

      expect(sanitized.stellarNetwork).toBe("testnet");
      expect(sanitized.contractId).toBe("C123");
      expect(sanitized.adminSecretKey).toBe("[REDACTED]");
      expect(sanitized.adminHmacSecret).toBe("[REDACTED]");
      expect(sanitized.apiKey).toBe("[REDACTED]");
      expect(sanitized.password).toBe("[REDACTED]");
      expect(sanitized.token).toBe("[REDACTED]");
    });
  });

  describe("Constructor & Dependency Injection", () => {
    it("should normalize single provider weight to 1.0", () => {
      const singleProviderConfig: OracleServiceConfig = {
        ...mockConfig,
        providers: [mockConfig.providers[0]], // only coingecko with weight 0.6
      };

      const service = new OracleService(singleProviderConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      const status = service.getStatus();
      expect(status.providers).toHaveLength(1);
      expect(status.providers[0].weight).toBe(1.0);
    });

    it("should clamp non-positive parameters gracefully during initialization", () => {
      const boundaryConfig: OracleServiceConfig = {
        ...mockConfig,
        maxPriceDeviationPercent: -1,
        priceStaleThresholdSeconds: 0,
        cacheTtlSeconds: -5,
        updateIntervalMs: -100,
      };

      expect(
        () =>
          new OracleService(boundaryConfig, {
            aggregator: mockAggregator,
            contractUpdater: mockUpdater,
          }),
      ).not.toThrow();
    });

    it("should instantiate through factory createOracleService", () => {
      const service = createOracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });
      expect(service).toBeInstanceOf(OracleService);
    });
  });

  describe("Lifecycle Invariants (start, stop, getStatus)", () => {
    it("should transition to running state and start periodic interval", async () => {
      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
        adminServer: mockAdminServer,
      });

      expect(service.getStatus().isRunning).toBe(false);

      await service.start(["XLM"]);

      expect(service.getStatus().isRunning).toBe(true);
      expect(mockAggregator.getPrices).toHaveBeenCalledWith(["XLM"]);
      expect(mockUpdater.updatePrices).toHaveBeenCalled();
      expect(mockAdminServer.start).toHaveBeenCalled();

      await service.stop();

      expect(service.getStatus().isRunning).toBe(false);
      expect(mockAdminServer.stop).toHaveBeenCalled();
    });

    it("should handle duplicate start calls idempotently", async () => {
      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      await service.start(["XLM"]);
      expect(service.getStatus().isRunning).toBe(true);

      // Second start should be safely ignored
      await service.start(["XLM"]);
      expect(service.getStatus().isRunning).toBe(true);

      await service.stop();
    });

    it("should handle duplicate stop calls idempotently", async () => {
      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      // Stop before start
      await service.stop();
      expect(service.getStatus().isRunning).toBe(false);

      await service.start(["XLM"]);
      await service.stop();
      await service.stop();
      expect(service.getStatus().isRunning).toBe(false);
    });

    it("should gracefully handle initial update failure during start without crashing service", async () => {
      mockAggregator.getPrices.mockRejectedValueOnce(
        new Error("Network failure"),
      );

      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      await expect(service.start(["XLM"])).resolves.not.toThrow();
      expect(service.getStatus().isRunning).toBe(true);

      await service.stop();
    });

    it("should clean up and propagate error if adminServer.start fails", async () => {
      mockAdminServer.start.mockRejectedValueOnce(
        new Error("Port already in use"),
      );

      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
        adminServer: mockAdminServer,
      });

      await expect(service.start(["XLM"])).rejects.toThrow(
        "Port already in use",
      );
      expect(service.getStatus().isRunning).toBe(false);
    });

    it("should return complete status via getStatus", () => {
      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      const status = service.getStatus();
      expect(status.isRunning).toBe(false);
      expect(status.isUpdating).toBe(false);
      expect(status.network).toBe("testnet");
      expect(status.contractId).toBe("CCONTRACT123");
      expect(status.providers).toEqual(mockConfig.providers);
      expect(status.aggregatorStats).toEqual({ cacheHits: 5, cacheMisses: 2 });
    });
  });

  describe("updatePrices Boundary & Adverse Conditions", () => {
    it("should reject non-array inputs with TypeError", async () => {
      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      await expect(service.updatePrices(null as any)).rejects.toThrow(
        TypeError,
      );
      await expect(service.updatePrices("XLM" as any)).rejects.toThrow(
        TypeError,
      );
      await expect(service.updatePrices(123 as any)).rejects.toThrow(TypeError);
    });

    it("should return empty array for empty assets list", async () => {
      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      const res = await service.updatePrices([]);
      expect(res).toEqual([]);
      expect(mockAggregator.getPrices).not.toHaveBeenCalled();
    });

    it("should deduplicate and sanitize asset symbols", async () => {
      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      await service.updatePrices([
        "xlm",
        "XLM",
        "  xlm  ",
        "",
        "   ",
        "btc",
        123 as any,
      ]);

      expect(mockAggregator.getPrices).toHaveBeenCalledWith(["XLM", "BTC"]);
    });

    it("should prevent overlapping concurrent update cycles", async () => {
      let resolvePrices: (val: any) => void;
      const pendingPromise = new Promise((resolve) => {
        resolvePrices = resolve;
      });
      mockAggregator.getPrices.mockReturnValueOnce(pendingPromise);

      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      // Start first update cycle (it will remain pending)
      const firstCycle = service.updatePrices(["XLM"]);
      expect(service.getStatus().isUpdating).toBe(true);

      // Second concurrent cycle should detect in-flight lock and skip execution
      const secondCycle = await service.updatePrices(["XLM"]);
      expect(secondCycle).toEqual([]);

      // Resolve first cycle
      resolvePrices!(
        new Map([
          [
            "XLM",
            {
              asset: "XLM",
              price: 150000n,
              timestamp: 1700000000,
              confidence: 95,
              sources: [],
            },
          ],
        ]),
      );

      const firstResult = await firstCycle;
      expect(firstResult).toHaveLength(1);
      expect(service.getStatus().isUpdating).toBe(false);
    });

    it("should handle complete aggregator failure gracefully", async () => {
      mockAggregator.getPrices.mockResolvedValueOnce(new Map());

      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      const results = await service.updatePrices(["XLM"]);
      expect(results).toEqual([]);
      expect(mockUpdater.updatePrices).not.toHaveBeenCalled();

      const status = service.getStatus();
      expect(status.lastUpdateResults?.failed).toBe(1);
      expect(status.lastUpdateResults?.successful).toBe(0);
    });

    it("should handle partial aggregator failures and submit successful assets", async () => {
      // Requested XLM and BTC, but only XLM returned
      mockAggregator.getPrices.mockResolvedValueOnce(
        new Map([
          [
            "XLM",
            {
              asset: "XLM",
              price: 150000n,
              timestamp: 1700000000,
              confidence: 95,
              sources: [],
            },
          ],
        ]),
      );

      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      const results = await service.updatePrices(["XLM", "BTC"]);
      expect(results).toHaveLength(1);
      expect(mockUpdater.updatePrices).toHaveBeenCalledWith([
        expect.objectContaining({ asset: "XLM" }),
      ]);
    });

    it("should support contractUpdater with submitPriceUpdate interface", async () => {
      const alternativeUpdater = {
        submitPriceUpdate: vi.fn().mockImplementation(async (priceData) => {
          if (priceData.asset === "FAIL") {
            throw new Error("On-chain submission rejected");
          }
          return { txHash: "tx-123" };
        }),
      };

      mockAggregator.getPrices.mockResolvedValueOnce(
        new Map([
          [
            "XLM",
            {
              asset: "XLM",
              price: 150000n,
              timestamp: 1700000000,
              confidence: 95,
              sources: [],
            },
          ],
          [
            "FAIL",
            {
              asset: "FAIL",
              price: 100n,
              timestamp: 1700000000,
              confidence: 90,
              sources: [],
            },
          ],
        ]),
      );

      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: alternativeUpdater as any,
      });

      const results = await service.updatePrices(["XLM", "FAIL"]);

      expect(results).toHaveLength(2);
      expect(results[0].success).toBe(true);
      expect(results[0].asset).toBe("XLM");
      expect(results[1].success).toBe(false);
      expect(results[1].asset).toBe("FAIL");
      expect(results[1].error).toContain("On-chain submission rejected");

      const status = service.getStatus();
      expect(status.lastUpdateResults).toEqual({
        total: 2,
        successful: 1,
        failed: 1,
      });
    });

    it("should catch unhandled errors in contractUpdater and return empty array", async () => {
      mockUpdater.updatePrices.mockRejectedValueOnce(
        new Error("Fatal RPC failure"),
      );

      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      const results = await service.updatePrices(["XLM"]);
      expect(results).toEqual([]);
      expect(service.getStatus().isUpdating).toBe(false);
    });
  });

  describe("fetchPrice Manual Query Boundaries", () => {
    it("should reject non-string or whitespace asset queries", async () => {
      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      await expect(service.fetchPrice(null as any)).rejects.toThrow(TypeError);
      await expect(service.fetchPrice("")).rejects.toThrow(TypeError);
      await expect(service.fetchPrice("   ")).rejects.toThrow(TypeError);
    });

    it("should query aggregator with normalized uppercase asset symbol", async () => {
      const service = new OracleService(mockConfig, {
        aggregator: mockAggregator,
        contractUpdater: mockUpdater,
      });

      const price = await service.fetchPrice("xlm");
      expect(mockAggregator.getPrice).toHaveBeenCalledWith("XLM");
      expect(price?.asset).toBe("XLM");
    });
  });
});
