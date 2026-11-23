/**
 * StellarLend API Usage Examples
 *
 * This file demonstrates how to interact with the StellarLend API
 * using the prepare-and-submit flow (client-side signing; secrets never leave the client).
 */

import axios, { AxiosError } from 'axios';

const API_BASE_URL = process.env.API_BASE_URL || 'http://localhost:3000/api';
const MAX_AMOUNT_STROOPS = 9223372036854775807n;

interface PreparedTransactionResponse {
  xdr: string;
  networkPassphrase: string;
}

interface TransactionResponse {
  success: boolean;
  transactionHash?: string;
  status: 'pending' | 'success' | 'failed';
  ledger?: number;
  message?: string;
  error?: string;
}

/**
 * Check API health status
 */
async function checkHealth(): Promise<void> {
  try {
    const response = await axios.get(`${API_BASE_URL}/health`);
    console.log('Health Check:', response.data.status === 'healthy' ? 'healthy' : 'unhealthy');

    if (response.data.status === 'healthy') {
      console.log('✅ All services are operational');
    } else {
      console.log('⚠️ One or more services are unavailable');
      throw new Error('API health check failed');
    }
  } catch (error) {
    handleError('Health check', error);
    throw error;
  }
}

/**
 * Prepare a deposit collateral transaction for client-side signing
 */
async function depositCollateral(
  userAddress: string,
  amount: string,
  assetAddress?: string
): Promise<PreparedTransactionResponse> {
  validateTransactionInput(userAddress, amount);
  try {
    console.log(`\n📥 Preparing deposit of ${amount} stroops...`);

    const response = await axios.post<PreparedTransactionResponse>(
      `${API_BASE_URL}/lending/deposit`,
      {
        userAddress,
        assetAddress,
        amount,
      }
    );

    console.log('✅ Deposit transaction prepared for client-side signing');
    return response.data;
  } catch (error) {
    handleError('Deposit', error);
    throw error;
  }
}

/**
 * Prepare a borrow assets transaction for client-side signing
 */
async function borrowAssets(
  userAddress: string,
  amount: string,
  assetAddress?: string
): Promise<PreparedTransactionResponse> {
  validateTransactionInput(userAddress, amount);
  try {
    console.log(`\n💰 Preparing borrow of ${amount} stroops...`);

    const response = await axios.post<PreparedTransactionResponse>(
      `${API_BASE_URL}/lending/borrow`,
      {
        userAddress,
        assetAddress,
        amount,
      }
    );

    console.log('✅ Borrow transaction prepared for client-side signing');
    return response.data;
  } catch (error) {
    handleError('Borrow', error);
    throw error;
  }
}

/**
 * Prepare a repay debt transaction for client-side signing
 */
async function repayDebt(
  userAddress: string,
  amount: string,
  assetAddress?: string
): Promise<PreparedTransactionResponse> {
  validateTransactionInput(userAddress, amount);
  try {
    console.log(`\n💳 Preparing repayment of ${amount} stroops...`);

    const response = await axios.post<PreparedTransactionResponse>(
      `${API_BASE_URL}/lending/repay`,
      {
        userAddress,
        assetAddress,
        amount,
      }
    );

    console.log('✅ Repay transaction prepared for client-side signing');
    return response.data;
  } catch (error) {
    handleError('Repay', error);
    throw error;
  }
}

/**
 * Prepare a withdraw collateral transaction for client-side signing
 */
async function withdrawCollateral(
  userAddress: string,
  amount: string,
  assetAddress?: string
): Promise<PreparedTransactionResponse> {
  validateTransactionInput(userAddress, amount);
  try {
    console.log(`\n📤 Preparing withdrawal of ${amount} stroops...`);

    const response = await axios.post<PreparedTransactionResponse>(
      `${API_BASE_URL}/lending/withdraw`,
      {
        userAddress,
        assetAddress,
        amount,
      }
    );

    console.log('✅ Withdraw transaction prepared for client-side signing');
    return response.data;
  } catch (error) {
    handleError('Withdraw', error);
    throw error;
  }
}

/**
 * Submit a client-signed transaction XDR
 */
async function submitSignedTransaction(
  signedXdr: string,
  authToken: string
): Promise<TransactionResponse> {
  if (typeof signedXdr !== 'string' || !signedXdr.trim()) {
    throw new Error('Signed transaction XDR is required');
  }
  if (typeof authToken !== 'string' || !authToken.trim()) {
    throw new Error('Auth token is required');
  }

  try {
    const response = await axios.post<TransactionResponse>(
      `${API_BASE_URL}/lending/submit`,
      { xdr: signedXdr },
      {
        headers: {
          Authorization: `Bearer ${authToken}`,
        },
      }
    );

    if (response.data.success && response.data.status === 'success') {
      console.log('✅ Transaction submitted!');
      console.log(`   Transaction Hash: ${response.data.transactionHash}`);
      console.log(`   Ledger: ${response.data.ledger}`);
    } else {
      console.log('❌ Transaction submission failed');
    }

    return response.data;
  } catch (error) {
    handleError('Submit', error);
    throw error;
  }
}

/**
 * Handle API errors
 */
function handleError(operation: string, error: unknown): void {
  if (axios.isAxiosError(error)) {
    const axiosError = error as AxiosError;
    if (axiosError.response) {
      console.error(`❌ ${operation} failed with HTTP ${axiosError.response.status}`);
    } else if (axiosError.request) {
      console.error(`❌ ${operation} failed: No response from server`);
    } else {
      console.error(`❌ ${operation} failed before the request was sent`);
    }
  } else {
    console.error(`❌ ${operation} failed`);
  }
}

function validateTransactionInput(userAddress: string, amount: string): void {
  if (typeof userAddress !== 'string' || !userAddress.trim()) {
    throw new Error('User address is required');
  }
  if (typeof amount !== 'string' || !/^[1-9]\d*$/.test(amount)) {
    throw new Error('Amount must be a positive whole number of stroops');
  }
  if (BigInt(amount) > MAX_AMOUNT_STROOPS) {
    throw new Error('Amount exceeds the maximum allowed stroops');
  }
}

/**
 * Complete lending lifecycle example (prepare step)
 */
async function completeLendingCycle(): Promise<void> {
  console.log('='.repeat(60));
  console.log('StellarLend API - Complete Lending Cycle Example');
  console.log('='.repeat(60));

  const USER_ADDRESS = 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX';

  try {
    await checkHealth();

    await depositCollateral(USER_ADDRESS, '100000000');
    await borrowAssets(USER_ADDRESS, '50000000');
    await repayDebt(USER_ADDRESS, '55000000');
    await withdrawCollateral(USER_ADDRESS, '50000000');

    console.log('\n' + '='.repeat(60));
    console.log('✅ All lending transactions prepared for client-side signing!');
    console.log('='.repeat(60));
  } catch (error) {
    console.log('\n' + '='.repeat(60));
    console.log('❌ Lending cycle failed');
    console.log('='.repeat(60));
    throw error;
  }
}

/**
 * Error handling examples
 */
async function errorHandlingExamples(): Promise<void> {
  console.log('\n' + '='.repeat(60));
  console.log('Error Handling Examples');
  console.log('='.repeat(60));

  const USER_ADDRESS = 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX';

  try {
    console.log('\n1. Testing zero amount (should fail)...');
    await depositCollateral(USER_ADDRESS, '0');
  } catch (error) {
    console.log('   Expected error caught ✓');
  }

  try {
    console.log('\n2. Testing invalid address (should fail)...');
    await depositCollateral('invalid_address', '1000000');
  } catch (error) {
    console.log('   Expected error caught ✓');
  }

  try {
    console.log('\n3. Testing unexpected extra field (should fail)...');
    await axios.post(`${API_BASE_URL}/lending/deposit`, {
      userAddress: USER_ADDRESS,
      amount: '1000000',
      unexpectedField: 'rejected',
    });
  } catch (error) {
    console.log('   Expected error caught ✓');
  }

  console.log('\n' + '='.repeat(60));
}

if (require.main === module) {
  const args = process.argv.slice(2);

  if (args.includes('--health')) {
    void checkHealth().catch(() => {
      process.exitCode = 1;
    });
  } else if (args.includes('--errors')) {
    void errorHandlingExamples().catch(() => {
      process.exitCode = 1;
    });
  } else if (args.includes('--cycle')) {
    void completeLendingCycle().catch(() => {
      process.exitCode = 1;
    });
  } else {
    console.log('Usage:');
    console.log('  ts-node examples/usage.ts --health   # Check API health');
    console.log('  ts-node examples/usage.ts --errors   # Test error handling');
    console.log('  ts-node examples/usage.ts --cycle    # Run complete cycle');
  }
}

export {
  checkHealth,
  depositCollateral,
  borrowAssets,
  repayDebt,
  withdrawCollateral,
  submitSignedTransaction,
  completeLendingCycle,
  errorHandlingExamples,
};
