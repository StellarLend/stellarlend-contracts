import { describe, it, expect, vi } from 'vitest';
import { Account, Keypair, xdr, nativeToScVal } from '@stellar/stellar-sdk';
import {
  SorobanAdapter,
  SorobanAdapterError,
  type SorobanRpcServerLike,
} from '../src/services/soroban-adapter';
import type { UpdateSubmission } from '../src/services/contract-updater';

const TEST_CONTRACT_ID = 'CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC';
const TEST_ASSET_CONTRACT_ID = 'CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM';

function makeSubmission(overrides: Partial<UpdateSubmission> = {}): UpdateSubmission {
  return {
    id: 'sub-1',
    idempotencyKey: 'sub-1',
    asset: TEST_ASSET_CONTRACT_ID,
    price: 0.123456,
    source: 'coingecko',
    observedAt: 1700000000000,
    createdAt: 1700000000000,
    status: 'PENDING',
    attempts: 1,
    expiresAt: 1700000120000,
    ...overrides,
  };
}

function makeFakeServer(overrides: Partial<SorobanRpcServerLike> = {}): SorobanRpcServerLike {
  return {
    getAccount: vi.fn().mockImplementation(async (address: string) => new Account(address, '100')),
    prepareTransaction: vi.fn().mockImplementation(async (tx) => tx),
    sendTransaction: vi.fn().mockResolvedValue({ status: 'PENDING', hash: 'tx-hash-success' }),
    getTransaction: vi.fn().mockResolvedValue({ status: 'SUCCESS' }),
    simulateTransaction: vi.fn().mockResolvedValue({
      result: { retval: xdr.ScVal.scvVoid() },
    }),
    ...overrides,
  };
}

describe('SorobanAdapter', () => {
  it('submits set_price with verified argument order, scales price, signs, and returns txHash', async () => {
    const admin = Keypair.random();
    let capturedArgs: xdr.ScVal[] = [];
    let capturedFnName = '';

    const server = makeFakeServer({
      prepareTransaction: vi.fn().mockImplementation(async (tx) => {
        const op = tx.operations[0] as unknown as {
          func: xdr.HostFunction;
        };
        const invoke = op.func.invokeContract();
        capturedFnName = invoke.functionName().toString();
        capturedArgs = invoke.args();
        return tx;
      }),
      getTransaction: vi
        .fn()
        .mockResolvedValueOnce({ status: 'NOT_FOUND' })
        .mockResolvedValueOnce({ status: 'SUCCESS' }),
    });

    const adapter = new SorobanAdapter({
      network: 'testnet',
      rpcUrl: 'https://soroban-testnet.stellar.org',
      contractId: TEST_CONTRACT_ID,
      adminSecretKey: admin.secret(),
      txPollTimeoutMs: 5_000,
      pollIntervalMs: 1,
      server,
    });

    const result = await adapter.submit(makeSubmission({ price: 1.5, observedAt: 1700000000000 }));

    expect(result).toEqual({ txHash: 'tx-hash-success' });
    expect(server.getAccount).toHaveBeenCalledWith(admin.publicKey());
    expect(server.prepareTransaction).toHaveBeenCalledTimes(1);
    expect(server.sendTransaction).toHaveBeenCalledTimes(1);
    expect(server.getTransaction).toHaveBeenCalledTimes(2);

    // Verify set_price(caller, asset, price, timestamp, signature)
    expect(capturedFnName).toBe('set_price');
    expect(capturedArgs).toHaveLength(5);
    expect(capturedArgs[0].switch()).toBe(xdr.ScValType.scvAddress());
    expect(capturedArgs[1].switch()).toBe(xdr.ScValType.scvAddress());
    expect(capturedArgs[2].switch()).toBe(xdr.ScValType.scvI128());
    expect(capturedArgs[3].switch()).toBe(xdr.ScValType.scvU64());
    expect(capturedArgs[3].u64().toString()).toBe('1700000000');
    expect(capturedArgs[4].switch()).toBe(xdr.ScValType.scvBytes());
    expect(capturedArgs[4].bytes().length).toBe(64);
  });

  it('throws a retryable coded error when transaction status is FAILED', async () => {
    const admin = Keypair.random();
    const server = makeFakeServer({
      getTransaction: vi.fn().mockResolvedValue({ status: 'FAILED' }),
    });

    const adapter = new SorobanAdapter({
      network: 'testnet',
      rpcUrl: 'https://soroban-testnet.stellar.org',
      contractId: TEST_CONTRACT_ID,
      adminSecretKey: admin.secret(),
      txPollTimeoutMs: 5_000,
      pollIntervalMs: 1,
      server,
    });

    await expect(adapter.submit(makeSubmission())).rejects.toMatchObject({
      name: 'SorobanAdapterError',
      code: 'TX_FAILED',
      retryable: true,
    });
  });

  it('throws a retryable TX_POLL_TIMEOUT error after txPollTimeoutMs elapses', async () => {
    const admin = Keypair.random();
    const server = makeFakeServer({
      getTransaction: vi.fn().mockResolvedValue({ status: 'NOT_FOUND' }),
    });

    const adapter = new SorobanAdapter({
      network: 'testnet',
      rpcUrl: 'https://soroban-testnet.stellar.org',
      contractId: TEST_CONTRACT_ID,
      adminSecretKey: admin.secret(),
      txPollTimeoutMs: 25,
      pollIntervalMs: 5,
      server,
    });

    await expect(adapter.submit(makeSubmission())).rejects.toMatchObject({
      name: 'SorobanAdapterError',
      code: 'TX_POLL_TIMEOUT',
      retryable: true,
    });
  });

  it('never includes adminSecretKey in error messages', async () => {
    const admin = Keypair.random();
    const secret = admin.secret();
    const server = makeFakeServer({
      sendTransaction: vi
        .fn()
        .mockRejectedValue(new Error(`RPC auth failed with secret ${secret}`)),
    });

    const adapter = new SorobanAdapter({
      network: 'testnet',
      rpcUrl: 'https://soroban-testnet.stellar.org',
      contractId: TEST_CONTRACT_ID,
      adminSecretKey: secret,
      txPollTimeoutMs: 5_000,
      pollIntervalMs: 1,
      server,
    });

    let caught: unknown;
    try {
      await adapter.submit(makeSubmission());
    } catch (err) {
      caught = err;
    }

    expect(caught).toBeInstanceOf(SorobanAdapterError);
    expect((caught as Error).message).not.toContain(secret);
    expect((caught as Error).message).toContain('[REDACTED]');
  });

  it('throws non-retryable INVALID_ASSET for invalid asset identifiers', async () => {
    const admin = Keypair.random();
    const server = makeFakeServer();

    const adapter = new SorobanAdapter({
      network: 'testnet',
      rpcUrl: 'https://soroban-testnet.stellar.org',
      contractId: TEST_CONTRACT_ID,
      adminSecretKey: admin.secret(),
      server,
    });

    await expect(
      adapter.submit(makeSubmission({ asset: 'THIS_ASSET_SYMBOL_IS_TOO_LONG' })),
    ).rejects.toMatchObject({
      name: 'SorobanAdapterError',
      code: 'INVALID_ASSET',
      retryable: false,
    });
  });

  it('reads latest on-chain price via get_price_record', async () => {
    const admin = Keypair.random();
    const priceRecordVal = nativeToScVal(
      { price: 1_500_000n, timestamp: 1700000000n },
      {
        type: {
          price: ['symbol', 'i128'],
          timestamp: ['symbol', 'u64'],
        },
      },
    );
    const server = makeFakeServer({
      simulateTransaction: vi.fn().mockResolvedValue({
        result: { retval: priceRecordVal },
      }),
    });

    const adapter = new SorobanAdapter({
      network: 'testnet',
      rpcUrl: 'https://soroban-testnet.stellar.org',
      contractId: TEST_CONTRACT_ID,
      adminSecretKey: admin.secret(),
      server,
    });

    const latest = await adapter.getLatestUpdate(TEST_ASSET_CONTRACT_ID);
    expect(latest).toEqual({
      price: 1_500_000n,
      timestamp: 1700000000000,
    });
    expect(server.simulateTransaction).toHaveBeenCalledTimes(1);
  });

  it('returns null from getLatestUpdate when get_price_record returns None/void', async () => {
    const admin = Keypair.random();
    const server = makeFakeServer({
      simulateTransaction: vi.fn().mockResolvedValue({
        result: { retval: xdr.ScVal.scvVoid() },
      }),
    });

    const adapter = new SorobanAdapter({
      network: 'testnet',
      rpcUrl: 'https://soroban-testnet.stellar.org',
      contractId: TEST_CONTRACT_ID,
      adminSecretKey: admin.secret(),
      server,
    });

    await expect(adapter.getLatestUpdate(TEST_ASSET_CONTRACT_ID)).resolves.toBeNull();
  });
});
