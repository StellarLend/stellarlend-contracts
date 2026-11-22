import { Request, Response, NextFunction } from 'express';
import jwt from 'jsonwebtoken';
import { Networks } from '@stellar/stellar-sdk';
import {
  authenticateToken,
  generateToken,
  verifyStellarSignature,
  verifyHookHmac,
  validateNetworkConsistency,
  requireAuthenticatedWallet,
  rateLimitByAddress,
  AuthRequest,
} from '../authorization';
import { config } from '../../config';
import { UnauthorizedError, ValidationError } from '../../utils/errors';

const VALID_ADDRESS = 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK';

function makeRes(): Partial<Response> & { setHeader: jest.Mock; headers: Record<string, string> } {
  const headers: Record<string, string> = {};
  const res: any = {
    headers,
    setHeader: jest.fn((key: string, value: string) => {
      headers[key] = value;
    }),
  };
  return res;
}

function makeReq(overrides: Partial<AuthRequest> = {}): AuthRequest {
  return { headers: {}, body: {}, ...overrides } as unknown as AuthRequest;
}

describe('authorization middleware', () => {
  afterEach(() => {
    jest.restoreAllMocks();
  });

  describe('authenticateToken', () => {
    it('rejects a request with no Authorization header', () => {
      const req = makeReq();
      const next = jest.fn();

      expect(() => authenticateToken(req, makeRes() as Response, next)).toThrow(
        UnauthorizedError
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('rejects a request with a malformed Authorization header', () => {
      const req = makeReq({ headers: { authorization: 'Bearer' } } as any);
      const next = jest.fn();

      expect(() => authenticateToken(req, makeRes() as Response, next)).toThrow(
        UnauthorizedError
      );
    });

    it('rejects a token signed with the wrong secret', () => {
      const forged = jwt.sign({ address: VALID_ADDRESS }, 'not-the-real-secret');
      const req = makeReq({ headers: { authorization: `Bearer ${forged}` } } as any);
      const next = jest.fn();

      expect(() => authenticateToken(req, makeRes() as Response, next)).toThrow(
        'Invalid or expired token'
      );
      expect(next).not.toHaveBeenCalled();
    });

    it('rejects an expired token', () => {
      const expired = jwt.sign({ address: VALID_ADDRESS }, config.auth.jwtSecret, {
        expiresIn: '-1s',
      } as jwt.SignOptions);
      const req = makeReq({ headers: { authorization: `Bearer ${expired}` } } as any);

      expect(() => authenticateToken(req, makeRes() as Response, jest.fn())).toThrow(
        'Invalid or expired token'
      );
    });

    it('rejects a structurally invalid token', () => {
      const req = makeReq({ headers: { authorization: 'Bearer not.a.jwt' } } as any);

      expect(() => authenticateToken(req, makeRes() as Response, jest.fn())).toThrow(
        'Invalid or expired token'
      );
    });

    it('accepts a valid token and attaches the decoded user', () => {
      const token = generateToken(VALID_ADDRESS, 'testnet');
      const req = makeReq({ headers: { authorization: `Bearer ${token}` } } as any);
      const next = jest.fn();

      authenticateToken(req, makeRes() as Response, next);

      expect(next).toHaveBeenCalled();
      // JWT also carries exp/iat; assert on the claims the middleware relies on.
      expect(req.user).toEqual(
        expect.objectContaining({ address: VALID_ADDRESS, network: 'testnet' })
      );
    });

    it('does not leak the signing secret in an error message', () => {
      const req = makeReq({ headers: { authorization: 'Bearer garbage' } } as any);

      try {
        authenticateToken(req, makeRes() as Response, jest.fn());
        fail('expected a throw');
      } catch (error) {
        expect((error as Error).message).not.toContain(config.auth.jwtSecret);
      }
    });
  });

  describe('generateToken', () => {
    it('round-trips address and network', () => {
      const token = generateToken(VALID_ADDRESS, 'public');
      const decoded = jwt.verify(token, config.auth.jwtSecret) as {
        address: string;
        network: string;
      };

      expect(decoded.address).toBe(VALID_ADDRESS);
      expect(decoded.network).toBe('public');
    });

    it('issues a token that authenticateToken accepts', () => {
      const token = generateToken(VALID_ADDRESS);
      const req = makeReq({ headers: { authorization: `Bearer ${token}` } } as any);
      const next = jest.fn();

      authenticateToken(req, makeRes() as Response, next);

      expect(next).toHaveBeenCalled();
    });
  });

  describe('verifyStellarSignature', () => {
    const run = (req: AuthRequest) => verifyStellarSignature(req, makeRes() as Response, jest.fn());

    it('rejects a request with no x-stellar-tx header', async () => {
      await expect(run(makeReq())).rejects.toThrow('x-stellar-tx');
    });

    it('rejects an unknown network', async () => {
      const req = makeReq({
        headers: { 'x-stellar-tx': 'AAAA', 'x-stellar-network': 'mainnet-does-not-exist' },
      } as any);

      await expect(run(req)).rejects.toThrow(ValidationError);
      await expect(run(req)).rejects.toThrow(/Invalid network/);
    });

    it('rejects a malformed transaction XDR', async () => {
      const req = makeReq({
        headers: { 'x-stellar-tx': 'not-valid-xdr', 'x-stellar-network': 'testnet' },
      } as any);

      await expect(run(req)).rejects.toThrow(UnauthorizedError);
    });

    it('rejects an XDR built for a different network than declared', async () => {
      // A real transaction XDR is required to reach the network comparison.
      const { Account, Keypair, Networks, TransactionBuilder, Operation, Asset } = require('@stellar/stellar-sdk');
      const source = new Account(VALID_ADDRESS, '0');
      const tx = new TransactionBuilder(source, {
        fee: '100',
        networkPassphrase: Networks.PUBLIC,
      })
        .addOperation(Operation.payment({ destination: VALID_ADDRESS, asset: Asset.native(), amount: '1' }))
        .setTimeout(300)
        .build()
        .toXDR();

      const req = makeReq({
        headers: { 'x-stellar-tx': tx, 'x-stellar-network': 'testnet' },
      } as any);

      await expect(run(req)).rejects.toThrow(UnauthorizedError);
    });

    describe('time bounds and signature verification', () => {
      const sdk = require('@stellar/stellar-sdk');
      const TESTNET = 'Test SDF Network ; September 2015';

      // A real keypair is required: the middleware verifies the transaction
      // hash against the source account's public key, so the signer must hold
      // the matching secret.
      const signer = sdk.Keypair.random();
      const sourceAddress = signer.publicKey();

      const buildTx = (opts: { sign?: boolean }) => {
        const source = new sdk.Account(sourceAddress, '0');
        const tx = new sdk.TransactionBuilder(source, {
          fee: '100',
          networkPassphrase: TESTNET,
        })
          .addOperation(
            sdk.Operation.payment({
              destination: VALID_ADDRESS,
              asset: sdk.Asset.native(),
              amount: '1',
            })
          )
          .setTimeout(300)
          .build();

        if (opts.sign) {
          tx.sign(signer);
        }
        return tx.toXDR();
      };

      it('rejects a transaction whose max time has passed', async () => {
        const txXdr = buildTx({});
        // `setTimeout` cannot express a max time in the past, so the clock is
        // moved past the transaction's 300s window instead.
        jest.spyOn(Date, 'now').mockReturnValue(Date.now() + 3600 * 1000);

        const req = makeReq({
          headers: { 'x-stellar-tx': txXdr, 'x-stellar-network': 'testnet' },
        } as any);

        await expect(run(req)).rejects.toThrow('Transaction has expired');
      });

      it('rejects an in-bounds transaction that carries no signature', async () => {
        const req = makeReq({
          headers: { 'x-stellar-tx': buildTx({}), 'x-stellar-network': 'testnet' },
        } as any);

        await expect(run(req)).rejects.toThrow('Transaction must be signed');
      });

      it('rejects a valid transaction signed by a different key', async () => {
        const attacker = sdk.Keypair.random();
        const source = new sdk.Account(sourceAddress, '0');
        const tx = new sdk.TransactionBuilder(source, { fee: '100', networkPassphrase: TESTNET })
          .addOperation(
            sdk.Operation.payment({
              destination: VALID_ADDRESS,
              asset: sdk.Asset.native(),
              amount: '1',
            })
          )
          .setTimeout(300)
          .build();
        tx.sign(attacker);

        const req = makeReq({
          headers: { 'x-stellar-tx': tx.toXDR(), 'x-stellar-network': 'testnet' },
        } as any);

        await expect(run(req)).rejects.toThrow('Invalid transaction signature');
      });

      it('accepts a correctly signed transaction and binds the signer', async () => {
        const txXdr = buildTx({ sign: true });
        const req = makeReq({
          headers: { 'x-stellar-tx': txXdr, 'x-stellar-network': 'testnet' },
        } as any);
        const next = jest.fn();

        await verifyStellarSignature(req, makeRes() as Response, next);

        expect(next).toHaveBeenCalled();
        expect(req.user).toEqual({ address: sourceAddress, network: 'testnet' });
        expect(req.validatedTransaction).toBeDefined();
      });

      it('defaults to testnet when no network header is supplied', async () => {
        const txXdr = buildTx({ sign: true });
        const req = makeReq({ headers: { 'x-stellar-tx': txXdr } } as any);
        const next = jest.fn();

        await verifyStellarSignature(req, makeRes() as Response, next);

        expect(req.user?.network).toBe('testnet');
      });
    });
  });

  describe('verifyHookHmac', () => {
    const secret = 'authorization-suite-hook-secret';
    let originalSecret: string;

    beforeAll(() => {
      originalSecret = config.auth.hookSecret;
      config.auth.hookSecret = secret;
    });

    afterAll(() => {
      config.auth.hookSecret = originalSecret;
    });

    const crypto = require('crypto');
    const body = { event: 'indexer.write', data: { id: '1' } };
    const rawBody = JSON.stringify(body);

    const signed = (timestamp: string, payload: string, key = secret) =>
      crypto.createHmac('sha256', key).update(`${timestamp}.${payload}`).digest('hex');

    it('rejects when no hook secret is configured', () => {
      const saved = config.auth.hookSecret;
      config.auth.hookSecret = '';
      try {
        const req = makeReq({
          headers: { 'x-hook-timestamp': '1', 'x-hook-signature': 'ab' },
        } as any);

        expect(() => verifyHookHmac(req, makeRes() as Response, jest.fn())).toThrow(
          'Hook authentication secret is not configured'
        );
      } finally {
        // Restore immediately: config is module state shared by later tests.
        config.auth.hookSecret = saved;
      }
    });

    it('rejects when signature and timestamp headers are missing', () => {
      const req = makeReq();

      expect(() => verifyHookHmac(req, makeRes() as Response, jest.fn())).toThrow(
        'Hook signature and timestamp headers are required'
      );
    });

    it('rejects a non-numeric timestamp', () => {
      const req = makeReq({
        headers: { 'x-hook-timestamp': 'soon', 'x-hook-signature': 'ab' },
      } as any);

      expect(() => verifyHookHmac(req, makeRes() as Response, jest.fn())).toThrow(
        'Invalid hook timestamp'
      );
    });

    it('rejects a timestamp outside the replay window', () => {
      const stale = String(Date.now() - 10 * 60 * 1000);
      const req = makeReq({
        headers: { 'x-hook-timestamp': stale, 'x-hook-signature': signed(stale, rawBody) },
      } as any);

      expect(() => verifyHookHmac(req, makeRes() as Response, jest.fn())).toThrow(
        /replay attack/i
      );
    });

    it('rejects a signature of the wrong length without throwing', () => {
      const timestamp = Date.now().toString();
      const req = makeReq({
        headers: { 'x-hook-timestamp': timestamp, 'x-hook-signature': 'abcd' },
      } as any);

      // A length mismatch must be rejected, never passed to timingSafeEqual.
      expect(() => verifyHookHmac(req, makeRes() as Response, jest.fn())).toThrow(
        /Invalid hook signature/
      );
    });

    it('rejects a signature computed with the wrong secret', () => {
      const timestamp = Date.now().toString();
      const req = makeReq({
        headers: {
          'x-hook-timestamp': timestamp,
          'x-hook-signature': signed(timestamp, rawBody, 'attacker-secret'),
        },
      } as any);

      expect(() => verifyHookHmac(req, makeRes() as Response, jest.fn())).toThrow(
        /Invalid hook signature/
      );
    });

    it('rejects a valid signature paired with a tampered raw body', () => {
      const timestamp = Date.now().toString();
      const req = makeReq({
        headers: { 'x-hook-timestamp': timestamp, 'x-hook-signature': signed(timestamp, rawBody) },
        body: { event: 'indexer.write', data: { id: 'tampered' } },
        // The signature is computed over the exact bytes on the wire. If those
        // bytes differ from what was signed, the request must be rejected.
        rawBody: JSON.stringify({ event: 'indexer.write', data: { id: 'tampered' } }),
      } as any);

      expect(() => verifyHookHmac(req, makeRes() as Response, jest.fn())).toThrow(
        /Invalid hook signature/
      );
    });

    it('rejects a signature replayed against a different body', () => {
      const timestamp = Date.now().toString();
      const attackerBody = JSON.stringify({ event: 'indexer.write', data: { id: 'attacker' } });
      const req = makeReq({
        headers: {
          'x-hook-timestamp': timestamp,
          'x-hook-signature': signed(timestamp, rawBody),
        },
        body: { event: 'indexer.write', data: { id: 'attacker' } },
        rawBody: attackerBody,
      } as any);

      expect(() => verifyHookHmac(req, makeRes() as Response, jest.fn())).toThrow(
        /Invalid hook signature/
      );
    });

    it('accepts a correctly signed request within the window', () => {
      const timestamp = Date.now().toString();
      const req = makeReq({
        headers: { 'x-hook-timestamp': timestamp, 'x-hook-signature': signed(timestamp, rawBody) },
        body,
        rawBody,
      } as any);
      const next = jest.fn();

      verifyHookHmac(req, makeRes() as Response, next);

      expect(next).toHaveBeenCalled();
    });

    it('accepts the first value of a repeated header', () => {
      const timestamp = Date.now().toString();
      const req = makeReq({
        headers: {
          'x-hook-timestamp': [timestamp, 'duplicate'],
          'x-hook-signature': [signed(timestamp, rawBody), 'duplicate'],
        },
        body,
        rawBody,
      } as any);
      const next = jest.fn();

      verifyHookHmac(req, makeRes() as Response, next);

      expect(next).toHaveBeenCalled();
    });
  });

  describe('validateNetworkConsistency', () => {
    it('rejects a request that specifies no network at all', () => {
      const req = makeReq();

      expect(() => validateNetworkConsistency(req, makeRes() as Response, jest.fn())).toThrow(
        'Network must be specified'
      );
    });

    it('rejects conflicting networks across header, session and body', () => {
      const req = makeReq({
        headers: { 'x-stellar-network': 'testnet' },
        body: { network: 'public' },
        user: { address: VALID_ADDRESS, network: 'futurenet' },
      } as any);

      expect(() => validateNetworkConsistency(req, makeRes() as Response, jest.fn())).toThrow(
        /Network mismatch/
      );
    });

    it('rejects an unsupported network name', () => {
      const req = makeReq({ headers: { 'x-stellar-network': 'devnet' } } as any);

      expect(() => validateNetworkConsistency(req, makeRes() as Response, jest.fn())).toThrow(
        /Invalid network/
      );
    });

    it('accepts a single consistent network and records it on the session', () => {
      const req = makeReq({
        headers: { 'x-stellar-network': 'testnet' },
        user: { address: VALID_ADDRESS },
      } as any);
      const next = jest.fn();

      validateNetworkConsistency(req, makeRes() as Response, next);

      expect(next).toHaveBeenCalled();
      expect(req.user?.network).toBe('testnet');
    });

    it('accepts repeated identical network indicators', () => {
      const req = makeReq({
        headers: { 'x-stellar-network': 'testnet' },
        body: { network: 'testnet' },
        user: { address: VALID_ADDRESS, network: 'testnet' },
      } as any);

      expect(() =>
        validateNetworkConsistency(req, makeRes() as Response, jest.fn())
      ).not.toThrow();
    });

    it('maps allowed networks to real Stellar passphrases', () => {
      expect(Networks.TESTNET).toContain('Test SDF Network');
    });
  });

  describe('requireAuthenticatedWallet', () => {
    it('rejects a request with no authenticated user', () => {
      const req = makeReq();

      expect(() => requireAuthenticatedWallet(req, makeRes() as Response, jest.fn())).toThrow(
        'Wallet must be connected and authenticated'
      );
    });

    it('rejects a user with no address', () => {
      const req = makeReq({ user: {} } as any);

      expect(() => requireAuthenticatedWallet(req, makeRes() as Response, jest.fn())).toThrow(
        UnauthorizedError
      );
    });

    it('rejects an address that does not start with G', () => {
      const req = makeReq({ user: { address: 'XBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQTNEQ5NWWK' } } as any);

      expect(() => requireAuthenticatedWallet(req, makeRes() as Response, jest.fn())).toThrow(
        'Invalid Stellar address format'
      );
    });

    it('rejects a truncated address of the wrong length', () => {
      const req = makeReq({ user: { address: 'GBO4N5HSFF3XMRRYYFGKNO6QEEIYCMDTFVUJUPNS2F5A7QQ' } } as any);

      expect(() => requireAuthenticatedWallet(req, makeRes() as Response, jest.fn())).toThrow(
        'Invalid Stellar address format'
      );
    });

    it('accepts a well-formed address', () => {
      const req = makeReq({ user: { address: VALID_ADDRESS } } as any);
      const next = jest.fn();

      requireAuthenticatedWallet(req, makeRes() as Response, next);

      expect(next).toHaveBeenCalled();
    });
  });

  describe('rateLimitByAddress', () => {
    // The rate limiter keys on the address string and its state lives in a
    // module-level map that outlives a single test, so each test uses its own
    // fixed address. Addresses are real StrKeys so the fixtures stay
    // well-formed, and the mapping test -> address never repeats.
    const crypto = require('crypto');
    const addressFor = (label: string) => {
      const hash = crypto.createHash('sha256').update(label).digest();
      const { StrKey } = require('@stellar/stellar-sdk');
      return StrKey.encodeEd25519PublicKey(hash);
    };

    it('rejects a request with no address', () => {
      const req = makeReq();

      expect(() => rateLimitByAddress(req, makeRes() as Response, jest.fn())).toThrow(
        'Address required for rate limiting'
      );
    });

    it('sets rate limit headers on the first request', () => {
      const address = addressFor('first-request');
      const req = makeReq({ user: { address } } as any);
      const res = makeRes();
      const next = jest.fn();

      rateLimitByAddress(req, res as Response, next);

      expect(next).toHaveBeenCalled();
      expect(res.headers['X-RateLimit-Limit']).toBe('100');
      expect(res.headers['X-RateLimit-Remaining']).toBe('99');
      expect(res.headers['X-RateLimit-Reset']).toBeDefined();
    });

    it('decrements the remaining count within the window', () => {
      const address = addressFor('decrement');
      const res = makeRes();
      const next = jest.fn();

      rateLimitByAddress(makeReq({ user: { address } } as any), res as Response, next);
      rateLimitByAddress(makeReq({ user: { address } } as any), res as Response, next);

      expect(res.headers['X-RateLimit-Remaining']).toBe('98');
    });

    it('tracks each address independently', () => {
      const a = addressFor('independent-a');
      const b = addressFor('independent-b');
      const res = makeRes();

      rateLimitByAddress(makeReq({ user: { address: a } } as any), res as Response, jest.fn());
      rateLimitByAddress(makeReq({ user: { address: a } } as any), res as Response, jest.fn());
      // A different address must not inherit the first address's usage.
      rateLimitByAddress(makeReq({ user: { address: b } } as any), res as Response, jest.fn());

      expect(res.headers['X-RateLimit-Remaining']).toBe('99');
    });

    it('rejects requests past the limit with Retry-After', () => {
      const address = addressFor('exhausted');
      const res = makeRes();
      const next = jest.fn();

      for (let i = 0; i < 100; i++) {
        rateLimitByAddress(makeReq({ user: { address } } as any), res as Response, next);
      }

      expect(() =>
        rateLimitByAddress(makeReq({ user: { address } } as any), res as Response, next)
      ).toThrow(/Rate limit exceeded/);
      expect(res.headers['Retry-After']).toBeDefined();
    });

    it('starts a fresh window once the previous one lapses', () => {
      const nowSpy = jest.spyOn(Date, 'now');
      const start = 1_000_000;
      nowSpy.mockReturnValue(start);

      const address = addressFor('fresh-window');
      const res = makeRes();
      const next = jest.fn();

      for (let i = 0; i < 100; i++) {
        rateLimitByAddress(makeReq({ user: { address } } as any), res as Response, next);
      }
      expect(() =>
        rateLimitByAddress(makeReq({ user: { address } } as any), res as Response, next)
      ).toThrow(/Rate limit exceeded/);

      // Move past the one-minute window.
      nowSpy.mockReturnValue(start + 61_000);
      rateLimitByAddress(makeReq({ user: { address } } as any), res as Response, next);

      expect(next).toHaveBeenCalled();
      expect(res.headers['X-RateLimit-Remaining']).toBe('99');
    });
  });
});
