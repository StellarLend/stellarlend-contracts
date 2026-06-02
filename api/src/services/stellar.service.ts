import {
  Keypair,
  Networks,
  TransactionBuilder,
  Operation,
  Asset,
  Account,
  BASE_FEE,
  Contract,
  xdr,
  Address,
  nativeToScVal,
} from '@stellar/stellar-sdk';
import { Server as SorobanServer } from '@stellar/stellar-sdk/rpc';
import axios from 'axios';
import { config } from '../config';
import logger from '../utils/logger';
import { InternalServerError } from '../utils/errors';
import {
  TransactionResponse,
  TransactionStatus,
  AmmEventDecodeResult,
  AmmEventKind,
  AmmEventTopic,
  AmmEventV1,
  AMM_EVENT_TOPIC_MODULE,
  AMM_EVENT_TOPIC_VERSION,
} from '../types';
import { SorobanRpc } from '@stellar/stellar-sdk';
import { Cursor } from '../utils/cursor';

/** Raw event from Soroban RPC */
interface RawContractEvent {
  id: string;
  type: string;
  ledger: number;
  ledgerClosedAt: string;
  contractId: string;
  topic: xdr.ScVal[];
  value: xdr.ScVal;
  inSuccessfulContractCall: boolean;
  txHash: string;
}

/** Parsed activity event */
interface ActivityEvent {
  id: string;
  type: 'borrow' | 'repay' | 'deposit' | 'withdraw' | 'liquidate';
  user: string;
  amount: string;
  asset: string;
  ledgerSequence: number;
  eventIndex: number;
  timestamp: string;
  txHash: string;
}

/** Parameters for ledger-range activity fetch */
interface FetchActivityParams {
  /** Starting ledger sequence (inclusive). null = from beginning */
  startLedger: number | null;
  /** Starting event index within the ledger (inclusive). null = from start of ledger */
  startEventIndex: number | null;
  /** Maximum events to return */
  limit: number;
}

/** Parameters for user-specific activity fetch */
interface FetchUserActivityParams extends FetchActivityParams {
  userAddress: string;
}

/** Service response with events and pagination metadata */
interface FetchActivityResult {
  events: ActivityEvent[];
  hasMore: boolean;
}


export class StellarService {
  private horizonUrl: string;
  private sorobanRpcUrl: string;
  private networkPassphrase: string;
  private contractId: string;
  private sorobanServer: SorobanServer;
  private sorobanBreaker: CircuitBreaker;
  private rpc: SorobanRpc.Server;
  private lendingContractId: string;

  constructor(rpcUrl: string, lendingContractId: string) {
    this.rpc = new SorobanRpc.Server(rpcUrl);
    this.lendingContractId = lendingContractId;
    this.horizonUrl = config.stellar.horizonUrl;
    this.sorobanRpcUrl = config.stellar.sorobanRpcUrl;
    this.networkPassphrase = config.stellar.networkPassphrase;
    this.contractId = config.stellar.contractId;
    this.sorobanServer = new SorobanServer(this.sorobanRpcUrl);
    this.sorobanBreaker = new CircuitBreaker({
      windowMs: config.circuitBreaker.windowMs,
      failureThreshold: config.circuitBreaker.failureThreshold,
      minRequests: config.circuitBreaker.minRequests,
      openMs: config.circuitBreaker.openMs,
      halfOpenMaxTrial: config.circuitBreaker.halfOpenMaxTrial,
    });
  }

  async getAccount(address: string): Promise<Account> {
    try {
      const response = await axios.get(`${this.horizonUrl}/accounts/${address}`);
      return new Account(response.data.id, response.data.sequence);
    } catch (error) {
      logger.error('Failed to fetch account:', error);
      throw new InternalServerError('Failed to fetch account information');
    }
  }

  async submitTransaction(txXdr: string): Promise<TransactionResponse> {
    try {
      const response = await axios.post(`${this.horizonUrl}/transactions`, {
        tx: txXdr,
      });

      return {
        success: true,
        transactionHash: response.data.hash,
        status: 'success',
        ledger: response.data.ledger,
      };
    } catch (error: any) {
      logger.error('Transaction submission failed:', error);
      return {
        success: false,
        status: 'failed',
        error: error.response?.data?.extras?.result_codes || error.message,
      };
    }
  }

  async buildDepositTransaction(
    userAddress: string,
    assetAddress: string | undefined,
    amount: string,
    userSecret: string
  ): Promise<string> {
    try {
      const sourceKeypair = Keypair.fromSecret(userSecret);
      const account = await this.getAccount(userAddress);

      const contract = new Contract(this.contractId);
      
      const params = [
        new Address(userAddress).toScVal(),
        assetAddress ? new Address(assetAddress).toScVal() : xdr.ScVal.scvVoid(),
        nativeToScVal(BigInt(amount), { type: 'i128' }),
      ];

      const operation = contract.call('deposit_collateral', ...params);

      const transaction = new TransactionBuilder(account, {
        fee: BASE_FEE,
        networkPassphrase: this.networkPassphrase,
      })
        .addOperation(operation)
        .setTimeout(30)
        .build();

      const preparedTx = await this.sorobanBreaker.exec(() =>
        this.sorobanServer.prepareTransaction(transaction)
      );
      preparedTx.sign(sourceKeypair);

      return preparedTx.toXDR();
    } catch (error) {
      logger.error('Failed to build deposit transaction:', error);
      throw new InternalServerError('Failed to build deposit transaction');
    }
  }

  async buildBorrowTransaction(
    userAddress: string,
    assetAddress: string | undefined,
    amount: string,
    userSecret: string
  ): Promise<string> {
    try {
      const sourceKeypair = Keypair.fromSecret(userSecret);
      const account = await this.getAccount(userAddress);

      const contract = new Contract(this.contractId);
      
      const params = [
        new Address(userAddress).toScVal(),
        assetAddress ? new Address(assetAddress).toScVal() : xdr.ScVal.scvVoid(),
        nativeToScVal(BigInt(amount), { type: 'i128' }),
      ];

      const operation = contract.call('borrow_asset', ...params);

      const transaction = new TransactionBuilder(account, {
        fee: BASE_FEE,
        networkPassphrase: this.networkPassphrase,
      })
        .addOperation(operation)
        .setTimeout(30)
        .build();

      const preparedTx = await this.sorobanBreaker.exec(() =>
        this.sorobanServer.prepareTransaction(transaction)
      );
      preparedTx.sign(sourceKeypair);

      return preparedTx.toXDR();
    } catch (error) {
      logger.error('Failed to build borrow transaction:', error);
      throw new InternalServerError('Failed to build borrow transaction');
    }
  }

  async buildRepayTransaction(
    userAddress: string,
    assetAddress: string | undefined,
    amount: string,
    userSecret: string
  ): Promise<string> {
    try {
      const sourceKeypair = Keypair.fromSecret(userSecret);
      const account = await this.getAccount(userAddress);

      const contract = new Contract(this.contractId);
      
      const params = [
        new Address(userAddress).toScVal(),
        assetAddress ? new Address(assetAddress).toScVal() : xdr.ScVal.scvVoid(),
        nativeToScVal(BigInt(amount), { type: 'i128' }),
      ];

      const operation = contract.call('repay_debt', ...params);

      const transaction = new TransactionBuilder(account, {
        fee: BASE_FEE,
        networkPassphrase: this.networkPassphrase,
      })
        .addOperation(operation)
        .setTimeout(30)
        .build();

      const preparedTx = await this.sorobanBreaker.exec(() =>
        this.sorobanServer.prepareTransaction(transaction)
      );
      preparedTx.sign(sourceKeypair);

      return preparedTx.toXDR();
    } catch (error) {
      logger.error('Failed to build repay transaction:', error);
      throw new InternalServerError('Failed to build repay transaction');
    }
  }

  async buildWithdrawTransaction(
    userAddress: string,
    assetAddress: string | undefined,
    amount: string,
    userSecret: string
  ): Promise<string> {
    try {
      const sourceKeypair = Keypair.fromSecret(userSecret);
      const account = await this.getAccount(userAddress);

      const contract = new Contract(this.contractId);
      
      const params = [
        new Address(userAddress).toScVal(),
        assetAddress ? new Address(assetAddress).toScVal() : xdr.ScVal.scvVoid(),
        nativeToScVal(BigInt(amount), { type: 'i128' }),
      ];

      const operation = contract.call('withdraw_collateral', ...params);

      const transaction = new TransactionBuilder(account, {
        fee: BASE_FEE,
        networkPassphrase: this.networkPassphrase,
      })
        .addOperation(operation)
        .setTimeout(30)
        .build();

      const preparedTx = await this.sorobanBreaker.exec(() =>
        this.sorobanServer.prepareTransaction(transaction)
      );
      preparedTx.sign(sourceKeypair);

      return preparedTx.toXDR();
    } catch (error) {
      logger.error('Failed to build withdraw transaction:', error);
      throw new InternalServerError('Failed to build withdraw transaction');
    }
  }

  async monitorTransaction(txHash: string, timeoutMs = 30000): Promise<TransactionResponse> {
    const startTime = Date.now();
    const pollInterval = 1000;

    while (Date.now() - startTime < timeoutMs) {
      try {
        const response = await axios.get(`${this.horizonUrl}/transactions/${txHash}`);
        
        if (response.data.successful) {
          return {
            success: true,
            transactionHash: txHash,
            status: 'success',
            ledger: response.data.ledger,
          };
        } else {
          return {
            success: false,
            transactionHash: txHash,
            status: 'failed',
            error: 'Transaction failed',
          };
        }
      } catch (error: any) {
        if (error.response?.status === 404) {
          await new Promise(resolve => setTimeout(resolve, pollInterval));
          continue;
        }
        
        logger.error('Error monitoring transaction:', error);
        throw new InternalServerError('Failed to monitor transaction');
      }
    }

    return {
      success: false,
      transactionHash: txHash,
      status: 'pending',
      message: 'Transaction monitoring timeout',
    };
  }

    /**
   * Fetch lending activity events by ledger range.
   *
   * Uses Soroban getEvents RPC with ledger sequence filtering.
   * Events are ordered by (ledgerSequence ASC, eventIndex ASC).
   *
   * @param params - Fetch parameters including cursor position and limit
   * @returns Parsed events and hasMore flag
   */
  async fetchActivityByLedgerRange(params: FetchActivityParams): Promise<FetchActivityResult> {
    const { startLedger, startEventIndex, limit } = params;

    // Determine start ledger for RPC call
    // If cursor provided, start from that ledger
    // Otherwise, use a reasonable lookback (e.g., last 1000 ledgers)
    const currentLedger = await this.getLatestLedger();
    const fromLedger = startLedger ?? Math.max(1, currentLedger - 1000);
    const toLedger = currentLedger;

    // Build event filters for lending contract
    const filters: SorobanRpc.EventFilter[] = [
      {
        type: 'contract',
        contractIds: [this.lendingContractId],
        topics: [
          ['*'], // Match all event topics (borrow, repay, deposit, withdraw, liquidate)
        ],
      },
    ];

    // Fetch events from RPC
    const response = await this.rpc.getEvents({
      startLedger: fromLedger,
      endLedger: toLedger,
      filters,
      limit,
    });

    // Parse and filter events
    const events = this.parseEvents(response.events ?? []);

    // Apply cursor offset if startEventIndex is specified
    // This handles the case where we need to skip events within a ledger
    let filteredEvents = events;
    if (startLedger !== null && startEventIndex !== null) {
      filteredEvents = events.filter((event) => {
        if (event.ledgerSequence < startLedger) return false;
        if (event.ledgerSequence === startLedger && event.eventIndex < startEventIndex) {
          return false;
        }
        return true;
      });
    }

    // Sort by (ledgerSequence, eventIndex) for stable ordering
    filteredEvents.sort((a, b) => {
      if (a.ledgerSequence !== b.ledgerSequence) {
        return a.ledgerSequence - b.ledgerSequence;
      }
      return a.eventIndex - b.eventIndex;
    });

    // Determine if there are more events
    // We requested `limit` events; if we got exactly `limit`, there may be more
    const hasMore = filteredEvents.length >= limit;

    return {
      events: filteredEvents.slice(0, limit),
      hasMore,
    };
  }

  /**
   * Fetch user-specific activity events by ledger range.
   *
   * Same pagination semantics as fetchActivityByLedgerRange but
   * filtered to events involving the specified user address.
   *
   * @param params - Fetch parameters including user address
   * @returns Filtered events and hasMore flag
   */
  async fetchUserActivityByLedgerRange(
    params: FetchUserActivityParams
  ): Promise<FetchActivityResult> {
    const { userAddress, startLedger, startEventIndex, limit } = params;

    // Fetch all activity first (same as general fetch)
    const { events, hasMore: generalHasMore } = await this.fetchActivityByLedgerRange({
      startLedger,
      startEventIndex,
      limit: limit * 2, // Fetch more to account for filtering
    });

    // Filter to user-specific events
    const userEvents = events.filter((event) => {
      // Match user address in event data
      // The exact matching depends on the event topic structure
      return event.user.toLowerCase() === userAddress.toLowerCase();
    });

    // If we filtered out too many, we might need to fetch more
    // For simplicity, we return what we have and let the client paginate
    const hasMore = generalHasMore || userEvents.length >= limit;

    return {
      events: userEvents.slice(0, limit),
      hasMore,
    };
  }


  /**
   * Get the latest closed ledger sequence from the RPC server.
   */
  private async getLatestLedger(): Promise<number> {
    const latest = await this.rpc.getLatestLedger();
    return latest.sequence;
  }

  /**
   * Parse raw Soroban events into structured ActivityEvents.
   *
   * Extracts event type, user address, amount, asset, and assigns
   * stable event indices within each ledger.
   */
  private parseEvents(rawEvents: RawContractEvent[]): ActivityEvent[] {
    // Group by ledger to assign event indices
    const ledgerGroups = new Map<number, RawContractEvent[]>();

    for (const event of rawEvents) {
      const existing = ledgerGroups.get(event.ledger) ?? [];
      existing.push(event);
      ledgerGroups.set(event.ledger, existing);
    }

    const parsed: ActivityEvent[] = [];

    for (const [ledgerSequence, events] of ledgerGroups) {
      // Sort events within ledger by their natural RPC order
      // This provides stable ordering within a ledger
      events.sort((a, b) => {
        // Use txHash + topic as tiebreaker for stable ordering
        const aKey = `${a.txHash}:${a.topic.map((t) => t.toXDR('hex')).join(':')}`;
        const bKey = `${b.txHash}:${b.topic.map((t) => t.toXDR('hex')).join(':')}`;
        return aKey.localeCompare(bKey);
      });

      for (let i = 0; i < events.length; i++) {
        const event = events[i];
        const parsedEvent = this.parseSingleEvent(event, ledgerSequence, i);
        if (parsedEvent) {
          parsed.push(parsedEvent);
        }
      }
    }

    return parsed;
  }

  /**
   * Parse a single raw event into an ActivityEvent.
   *
   * @param event - Raw RPC event
   * @param ledgerSequence - The ledger this event belongs to
   * @param eventIndex - Stable index within the ledger
   * @returns Parsed event or null if unparseable
   */
  private parseSingleEvent(
    event: RawContractEvent,
    ledgerSequence: number,
    eventIndex: number
  ): ActivityEvent | null {
    try {
      // Topic[0] is the event type symbol
      const eventType = this.parseEventType(event.topic[0]);
      if (!eventType) return null;

      // Topic[1] is typically the user address
      const user = this.parseAddress(event.topic[1]);

      // Value contains amount and asset
      const { amount, asset } = this.parseEventValue(event.value);

      return {
        id: event.id,
        type: eventType,
        user,
        amount,
        asset,
        ledgerSequence,
        eventIndex,
        timestamp: event.ledgerClosedAt,
        txHash: event.txHash,
      };
    } catch (error) {
      console.warn('Failed to parse event:', event.id, error);
      return null;
    }
  }

  /**
   * Parse event type from topic ScVal.
   */
  private parseEventType(topicVal: xdr.ScVal): ActivityEvent['type'] | null {
    try {
      const sym = topicVal.sym().toString();
      const validTypes: ActivityEvent['type'][] = ['borrow', 'repay', 'deposit', 'withdraw', 'liquidate'];
      if (validTypes.includes(sym as ActivityEvent['type'])) {
        return sym as ActivityEvent['type'];
      }
      return null;
    } catch {
      return null;
    }
  }

  /**
   * Parse address from ScVal.
   */
  private parseAddress(val: xdr.ScVal | undefined): string {
    if (!val) return '';
    try {
      return val.address().toString();
    } catch {
      return '';
    }
  }

  /**
   * Parse amount and asset from event value ScVal.
   */
  private parseEventValue(val: xdr.ScVal): { amount: string; asset: string } {
    try {
      // Assuming value is a Map with 'amount' and 'asset' keys
      // Adjust based on actual contract event structure
      const map = val.map();
      let amount = '0';
      let asset = '';

      for (const entry of map) {
        const key = entry.key().sym().toString();
        if (key === 'amount') {
          amount = entry.val().i128().lo().toString();
        } else if (key === 'asset') {
          asset = entry.val().address().toString();
        }
      }

      return { amount, asset };
    } catch {
      return { amount: '0', asset: '' };
    }

  public parseAmmEventTopic(topics: unknown): AmmEventTopic | null {
    if (!Array.isArray(topics) || topics.length !== 3) {
      return null;
    }

    const [module, version, kind] = topics;
    if (
      module !== AMM_EVENT_TOPIC_MODULE ||
      version !== AMM_EVENT_TOPIC_VERSION ||
      typeof kind !== 'string'
    ) {
      return null;
    }

    const eventKind = kind as AmmEventKind;
    if (!['swap', 'add_liquidity', 'remove_liquidity'].includes(eventKind)) {
      return null;
    }

    return {
      module: AMM_EVENT_TOPIC_MODULE,
      version: AMM_EVENT_TOPIC_VERSION,
      kind: eventKind,
    };
  }

  public decodeAmmEvent(rawEvent: unknown): AmmEventDecodeResult | null {
    if (!rawEvent || typeof rawEvent !== 'object') {
      return null;
    }

    const event = rawEvent as { topics?: unknown; data?: unknown };
    const topic = this.parseAmmEventTopic(event.topics);
    if (!topic) {
      return null;
    }

    if (!event.data || typeof event.data !== 'object') {
      return null;
    }

    const data = event.data as unknown as AmmEventV1;
    if (data.schema_version !== 1 || data.event !== topic.kind) {
      return null;
    }

    return {
      topic,
      data,
    };
  }

  public extractAmmEventsFromTransactionResult(txResult: any): AmmEventDecodeResult[] {
    if (!txResult || !Array.isArray(txResult.events)) {
      return [];
    }

    return txResult.events
      .map((event: unknown): AmmEventDecodeResult | null => this.decodeAmmEvent(event))
      .filter(
        (decoded: AmmEventDecodeResult | null): decoded is AmmEventDecodeResult =>
          decoded !== null
      );
  }

  async healthCheck(): Promise<{ horizon: boolean; sorobanRpc: boolean }> {
    const results = {
      horizon: false,
      sorobanRpc: false,
    };

    try {
      await axios.get(`${this.horizonUrl}/`);
      results.horizon = true;
    } catch (error) {
      logger.error('Horizon health check failed:', error);
    }

    try {
      // If circuit is open, treat soroban RPC as unhealthy immediately
      const breakerState = this.sorobanBreaker.getState();
      if (breakerState === 'OPEN') {
        results.sorobanRpc = false;
      } else {
        await this.sorobanBreaker.exec(() => this.sorobanServer.getHealth());
        results.sorobanRpc = true;
      }
    } catch (error) {
      logger.error('Soroban RPC health check failed:', error);
    }

    // attach breaker metrics for observability
    (results as any).sorobanBreaker = this.sorobanBreaker.getMetrics();

    return results;
  }

  /**
   * Ping the soroban RPC and attempt a lightweight contract invocation
   * to verify contract reachability. Returns rpc, contract and ledger info.
   */
  async pingContract(): Promise<{ rpc: boolean; contract: boolean; ledger: number | null }> {
    const status = { rpc: false, contract: false, ledger: null as number | null };

    // Check RPC health
    try {
      await this.sorobanServer.getHealth();
      status.rpc = true;
    } catch (error) {
      logger.error('Soroban RPC health check failed (pingContract):', error);
      // If RPC is down we cannot proceed to contract check
      return status;
    }

    // Try to fetch latest ledger from Horizon for diagnostic info
    try {
      const resp = await axios.get(`${this.horizonUrl}/ledgers?order=desc&limit=1`);
      const latest = resp.data?._embedded?.records?.[0];
      if (latest && latest.sequence) {
        status.ledger = Number(latest.sequence);
      }
    } catch (error) {
      logger.warn('Failed to fetch latest ledger for health check:', error);
    }

    // Attempt a lightweight contract invocation via prepareTransaction.
    // This will exercise the Soroban RPC path for invoking the named
    // function and will fail if the contract or RPC cannot be reached.
    try {
      const tempKey = Keypair.random().publicKey();
      const account = new Account(tempKey, '1');
      const contract = new Contract(this.contractId);
      const operation = contract.call('get_admin');

      const tx = new TransactionBuilder(account, {
        fee: BASE_FEE,
        networkPassphrase: this.networkPassphrase,
      })
        .addOperation(operation)
        .setTimeout(10)
        .build();

      // prepareTransaction will call out to the soroban RPC; success implies
      // the contract is reachable and callable (at least for read-only).
      await this.sorobanServer.prepareTransaction(tx);
      status.contract = true;
    } catch (error) {
      logger.error('Contract ping failed (pingContract):', error);
    }

    return status;
  }
}

