/**
 * Contract Updater Service
*/

import {
    Keypair,
    Contract,
    SorobanRpc,
    TransactionBuilder,
    Networks,
    xdr,
    Address,
    nativeToScVal,
} from '@stellar/stellar-sdk';
import type { ContractUpdateResult, AggregatedPrice } from '../types/index.js';
import { logger } from '../utils/logger.js';

/**
 * Contract updater configuration
 */
export interface ContractUpdaterConfig {
    network: 'testnet' | 'mainnet';
    rpcUrl: string;
    /** StellarLend contract ID */
    contractId: string;
    /** Admin secret key for signing */
    adminSecretKey: string;
    maxRetries: number;
    retryDelayMs: number;
}

/**
 * Default configuration
 */
const DEFAULT_CONFIG: Partial<ContractUpdaterConfig> = {
    maxRetries: 3,
    retryDelayMs: 1000,
};

/**
 * Contract Updater
 */
export class ContractUpdater {
    private config: ContractUpdaterConfig;
    private server: SorobanRpc.Server;
    private adminKeypair: Keypair;
    private networkPassphrase: string;

    constructor(config: ContractUpdaterConfig) {
        this.config = { ...DEFAULT_CONFIG, ...config } as ContractUpdaterConfig;

        this.server = new SorobanRpc.Server(this.config.rpcUrl);
        this.adminKeypair = Keypair.fromSecret(this.config.adminSecretKey);
        this.networkPassphrase = this.config.network === 'testnet'
            ? Networks.TESTNET
            : Networks.PUBLIC;

        logger.info('Contract updater initialized', {
            network: this.config.network,
            contractId: this.config.contractId,
            adminPublicKey: this.adminKeypair.publicKey(),
        });
    }

    /**
     * Update price for a single asset
     */
    async updatePrice(
        asset: string,
        price: bigint,
        timestamp: number,
    ): Promise<ContractUpdateResult> {
        const startTime = Date.now();
        let lastError: Error | undefined;

        for (let attempt = 1; attempt <= this.config.maxRetries; attempt++) {
            try {
                logger.info(`Updating price for ${asset} (attempt ${attempt})`, {
                    price: price.toString(),
                    timestamp,
                });

                const txHash = await this.submitPriceUpdate(asset, price, timestamp);

                const result: ContractUpdateResult = {
                    success: true,
                    transactionHash: txHash,
                    asset,
                    price,
                    timestamp,
                };

                logger.info(`Price update successful for ${asset}`, {
                    txHash,
                    durationMs: Date.now() - startTime,
                });

                return result;
            } catch (error) {
                lastError = error instanceof Error ? error : new Error(String(error));

                logger.warn(`Price update attempt ${attempt} failed for ${asset}`, {
                    error: lastError.message,
                });

                if (attempt < this.config.maxRetries) {
                    const delay = this.config.retryDelayMs * Math.pow(2, attempt - 1);
                    await this.sleep(delay);
                }
            }
        }

        logger.error(`All price update attempts failed for ${asset}`, {
            error: lastError?.message,
        });

        return {
            success: false,
            asset,
            price,
            timestamp,
            error: lastError?.message || 'Unknown error',
        };
    }

    /**
     * Update prices for multiple assets
     */
    async updatePrices(
        prices: AggregatedPrice[],
    ): Promise<ContractUpdateResult[]> {
        const results: ContractUpdateResult[] = [];

        for (const price of prices) {
            const result = await this.updatePrice(
                price.asset,
                price.price,
                price.timestamp,
            );
            results.push(result);

            await this.sleep(100);
        }

        return results;
    }

    /**
     * Submit a price update transaction to the contract
     */
    private async submitPriceUpdate(
        asset: string,
        price: bigint,
        timestamp: number,
    ): Promise<string> {
        const contract = new Contract(this.config.contractId);
        const adminAddress = new Address(this.adminKeypair.publicKey());

        const operation = contract.call(
            'set_asset_price',
            adminAddress.toScVal(),
            xdr.ScVal.scvSymbol(asset),
            nativeToScVal(price, { type: 'i128' }),
            nativeToScVal(timestamp, { type: 'u64' }),
        );

        const account = await this.server.getAccount(this.adminKeypair.publicKey());

        const transaction = new TransactionBuilder(account, {
            fee: '100000',
            networkPassphrase: this.networkPassphrase,
        })
            .addOperation(operation)
            .setTimeout(30)
            .build();

        const simulated = await this.server.simulateTransaction(transaction);

        if (SorobanRpc.Api.isSimulationError(simulated)) {
            throw new Error(`Simulation failed: ${simulated.error}`);
        }

        if (!SorobanRpc.Api.isSimulationSuccess(simulated)) {
            throw new Error('Simulation did not succeed');
        }

        const prepared = SorobanRpc.assembleTransaction(transaction, simulated).build();
        prepared.sign(this.adminKeypair);

        const response = await this.server.sendTransaction(prepared);

        if (response.status === 'ERROR') {
            throw new Error(`Transaction failed: ${response.errorResult}`);
        }

        const hash = response.hash;
        let getResponse = await this.server.getTransaction(hash);

        while (getResponse.status === SorobanRpc.Api.GetTransactionStatus.NOT_FOUND) {
            await this.sleep(1000);
            getResponse = await this.server.getTransaction(hash);
        }

        if (getResponse.status === SorobanRpc.Api.GetTransactionStatus.FAILED) {
            throw new Error(`Transaction failed on-chain`);
        }

        return hash;
    }

    /**
     * Check if the contract is accessible
     */
    async healthCheck(): Promise<boolean> {
        try {
            const contract = new Contract(this.config.contractId);
            return !!contract;
        } catch {
            return false;
        }
    }

    /**
     * Get the admin public key
     */
    getAdminPublicKey(): string {
        return this.adminKeypair.publicKey();
    }

    /**
     * Sleep utility
     */
    private sleep(ms: number): Promise<void> {
        return new Promise((resolve) => setTimeout(resolve, ms));
    }
}

/**
 * Create a contract updater
 */
export function createContractUpdater(
    config: ContractUpdaterConfig,
): ContractUpdater {
    return new ContractUpdater(config);
}
