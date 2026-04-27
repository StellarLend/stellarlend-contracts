/**
 * StellarLend API Usage Examples
 * 
 * This file demonstrates how to interact with the StellarLend API
 * for common lending operations.
 */

import axios, { AxiosError } from 'axios';

const API_BASE_URL = process.env.API_BASE_URL || 'http://localhost:3000/api';

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
    console.log('Health Check:', response.data);
    
    if (response.data.status === 'healthy') {
      console.log('✅ All services are operational');
    } else {
      console.log('⚠️ Some services are down:', response.data.services);
    }
  } catch (error) {
    console.error('❌ Health check failed:', error);
  }
}

/**
 * Deposit collateral into the lending protocol
 */
async function depositCollateral(
  userAddress: string,
  amount: string,
  userSecret: string,
  assetAddress?: string
): Promise<TransactionResponse> {
  try {
    console.log(`\n📥 Depositing ${amount} stroops...`);
    
    const response = await axios.post<TransactionResponse>(
      `${API_BASE_URL}/lending/deposit`,
      {
        userAddress,
        assetAddress,
        amount,
        userSecret,
      }
    );

    if (response.data.success) {
      console.log('✅ Deposit successful!');
      console.log(`   Transaction Hash: ${response.data.transactionHash}`);
      console.log(`   Ledger: ${response.data.ledger}`);
    } else {
      console.log('❌ Deposit failed:', response.data.error);
    }

    return response.data;
  } catch (error) {
    handleError('Deposit', error);
    throw error;
  }
}

/**
 * Borrow assets against deposited collateral
 */
async function borrowAssets(
  userAddress: string,
  amount: string,
  userSecret: string,
  assetAddress?: string
): Promise<TransactionResponse> {
  try {
    console.log(`\n💰 Borrowing ${amount} stroops...`);
    
    const response = await axios.post<TransactionResponse>(
      `${API_BASE_URL}/lending/borrow`,
      {
        userAddress,
        assetAddress,
        amount,
        userSecret,
      }
    );

    if (response.data.success) {
      console.log('✅ Borrow successful!');
      console.log(`   Transaction Hash: ${response.data.transactionHash}`);
      console.log(`   Ledger: ${response.data.ledger}`);
    } else {
      console.log('❌ Borrow failed:', response.data.error);
    }

    return response.data;
  } catch (error) {
    handleError('Borrow', error);
    throw error;
  }
}

/**
 * Repay borrowed assets with interest
 */
async function repayDebt(
  userAddress: string,
  amount: string,
  userSecret: string,
  assetAddress?: string
): Promise<TransactionResponse> {
  try {
    console.log(`\n💳 Repaying ${amount} stroops...`);
    
    const response = await axios.post<TransactionResponse>(
      `${API_BASE_URL}/lending/repay`,
      {
        userAddress,
        assetAddress,
        amount,
        userSecret,
      }
    );

    if (response.data.success) {
      console.log('✅ Repayment successful!');
      console.log(`   Transaction Hash: ${response.data.transactionHash}`);
      console.log(`   Ledger: ${response.data.ledger}`);
    } else {
      console.log('❌ Repayment failed:', response.data.error);
    }

    return response.data;
  } catch (error) {
    handleError('Repay', error);
    throw error;
  }
}

/**
 * Withdraw collateral from the protocol
 */
async function withdrawCollateral(
  userAddress: string,
  amount: string,
  userSecret: string,
  assetAddress?: string
): Promise<TransactionResponse> {
  try {
    console.log(`\n📤 Withdrawing ${amount} stroops...`);
    
    const response = await axios.post<TransactionResponse>(
      `${API_BASE_URL}/lending/withdraw`,
      {
        userAddress,
        assetAddress,
        amount,
        userSecret,
      }
    );

    if (response.data.success) {
      console.log('✅ Withdrawal successful!');
      console.log(`   Transaction Hash: ${response.data.transactionHash}`);
      console.log(`   Ledger: ${response.data.ledger}`);
    } else {
      console.log('❌ Withdrawal failed:', response.data.error);
    }

    return response.data;
  } catch (error) {
    handleError('Withdraw', error);
    throw error;
  }
}

/**
 * Handle API errors
 */
function handleError(operation: string, error: unknown): void {
  if (axios.isAxiosError(error)) {
    const axiosError = error as AxiosError<{ error: string }>;
    if (axiosError.response) {
      console.error(`❌ ${operation} failed:`, axiosError.response.data.error);
      console.error(`   Status: ${axiosError.response.status}`);
    } else if (axiosError.request) {
      console.error(`❌ ${operation} failed: No response from server`);
    } else {
      console.error(`❌ ${operation} failed:`, axiosError.message);
    }
  } else {
    console.error(`❌ ${operation} failed:`, error);
  }
}

/**
 * Complete lending lifecycle example
 */
async function completeLendingCycle(): Promise<void> {
  console.log('='.repeat(60));
  console.log('StellarLend API - Complete Lending Cycle Example');
  console.log('='.repeat(60));

  // Replace with your actual testnet credentials
  const USER_ADDRESS = 'GXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX';
  const USER_SECRET = 'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX';
  
  try {
    // 1. Check health
    await checkHealth();

    // 2. Deposit collateral (10 XLM)
    await depositCollateral(USER_ADDRESS, '100000000', USER_SECRET);

    // Wait a bit for transaction to settle
    await new Promise(resolve => setTimeout(resolve, 5000));

    // 3. Borrow assets (5 XLM)
    await borrowAssets(USER_ADDRESS, '50000000', USER_SECRET);

    // Wait a bit for transaction to settle
    await new Promise(resolve => setTimeout(resolve, 5000));

    // 4. Repay debt (5.5 XLM with interest)
    await repayDebt(USER_ADDRESS, '55000000', USER_SECRET);

    // Wait a bit for transaction to settle
    await new Promise(resolve => setTimeout(resolve, 5000));

    // 5. Withdraw collateral (5 XLM)
    await withdrawCollateral(USER_ADDRESS, '50000000', USER_SECRET);

    console.log('\n' + '='.repeat(60));
    console.log('✅ Complete lending cycle finished successfully!');
    console.log('='.repeat(60));
  } catch (error) {
    console.log('\n' + '='.repeat(60));
    console.log('❌ Lending cycle failed');
    console.log('='.repeat(60));
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
  const USER_SECRET = 'SXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX';

  // Example 1: Invalid amount (zero)
  try {
    console.log('\n1. Testing zero amount (should fail)...');
    await depositCollateral(USER_ADDRESS, '0', USER_SECRET);
  } catch (error) {
    console.log('   Expected error caught ✓');
  }

  // Example 2: Invalid address
  try {
    console.log('\n2. Testing invalid address (should fail)...');
    await depositCollateral('invalid_address', '1000000', USER_SECRET);
  } catch (error) {
    console.log('   Expected error caught ✓');
  }

  // Example 3: Missing required field
  try {
    console.log('\n3. Testing missing secret (should fail)...');
    await axios.post(`${API_BASE_URL}/lending/deposit`, {
      userAddress: USER_ADDRESS,
      amount: '1000000',
      // userSecret missing
    });
  } catch (error) {
    console.log('   Expected error caught ✓');
  }

  console.log('\n' + '='.repeat(60));
}

// Run examples if executed directly
if (require.main === module) {
  const args = process.argv.slice(2);
  
  if (args.includes('--health')) {
    checkHealth();
  } else if (args.includes('--errors')) {
    errorHandlingExamples();
  } else if (args.includes('--cycle')) {
    completeLendingCycle();
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
  completeLendingCycle,
  errorHandlingExamples,
};
