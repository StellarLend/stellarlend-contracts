import axios, { AxiosError } from 'axios';
import {
  borrowAssets,
  checkHealth,
  depositCollateral,
  repayDebt,
  withdrawCollateral,
} from '../../examples/usage';

describe('API usage examples', () => {
  const userAddress = 'GUSER';
  const userSecret = 'SENSITIVE_USER_SECRET';
  const apiBaseUrl = process.env.API_BASE_URL || 'http://localhost:3000/api';
  const endpoint = `${apiBaseUrl}/lending/deposit`;

  let postSpy: jest.SpyInstance;
  let errorSpy: jest.SpyInstance;
  let logSpy: jest.SpyInstance;

  beforeEach(() => {
    postSpy = jest.spyOn(axios, 'post');
    errorSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined);
    logSpy = jest.spyOn(console, 'log').mockImplementation(() => undefined);
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  it('sends transaction inputs and returns a successful response', async () => {
    const result = {
      success: true,
      status: 'success' as const,
      transactionHash: 'tx-hash',
      ledger: 42,
    };
    postSpy.mockResolvedValue({ data: result });

    await expect(
      depositCollateral(userAddress, '1000000', userSecret, 'CASHSET'),
    ).resolves.toEqual(result);

    expect(postSpy).toHaveBeenCalledWith(endpoint, {
      userAddress,
      assetAddress: 'CASHSET',
      amount: '1000000',
      userSecret,
    });
    expect(logSpy).toHaveBeenCalledWith('✅ Deposit successful!');
  });

  it('returns API-declared failure responses without treating them as transport errors', async () => {
    const result = {
      success: false,
      status: 'failed' as const,
      error: `Invalid amount ${userSecret}`,
    };
    postSpy.mockResolvedValue({ data: result, status: 422 });

    await expect(
      depositCollateral(userAddress, '0', userSecret),
    ).resolves.toEqual(result);

    expect(postSpy).toHaveBeenCalledWith(endpoint, {
      userAddress,
      assetAddress: undefined,
      amount: '0',
      userSecret,
    });
    const logged = JSON.stringify(errorSpy.mock.calls);
    expect(logged).toContain('API reported failure');
    expect(logged).toContain('422');
    expect(logged).not.toContain(userSecret);
  });

  it.each([
    ['Borrow', '/lending/borrow', borrowAssets],
    ['Repay', '/lending/repay', repayDebt],
    ['Withdraw', '/lending/withdraw', withdrawCollateral],
  ])('redacts API failure bodies for %s', async (operation, path, run) => {
    postSpy.mockResolvedValue({
      data: { success: false, status: 'failed', error: userSecret },
      status: 503,
    });

    await run(userAddress, '1', userSecret);

    const logged = JSON.stringify(errorSpy.mock.calls);
    expect(logged).toContain(operation);
    expect(logged).toContain('503');
    expect(logged).not.toContain(userSecret);
    expect(postSpy).toHaveBeenCalledWith(
      `${apiBaseUrl}${path}`,
      expect.objectContaining({ userAddress, amount: '1', userSecret }),
    );
  });

  it('rethrows network failures and logs safe diagnostics only', async () => {
    const failure = Object.assign(
      new AxiosError(`request included ${userSecret}`, 'ECONNRESET'),
      { request: {} },
    );
    postSpy.mockRejectedValue(failure);

    await expect(
      depositCollateral(userAddress, '1000000', userSecret),
    ).rejects.toBe(failure);

    const logged = JSON.stringify(errorSpy.mock.calls);
    expect(logged).toContain('No response from server');
    expect(logged).not.toContain(userSecret);
  });

  it('does not log response bodies that may contain credentials', async () => {
    const failure = Object.assign(new AxiosError('Request failed', 'ERR_BAD_RESPONSE'), {
      response: {
        status: 502,
        data: { error: `invalid credential ${userSecret}` },
      },
    });
    postSpy.mockRejectedValue(failure);

    await expect(
      depositCollateral(userAddress, '1000000', userSecret),
    ).rejects.toBe(failure);

    const logged = JSON.stringify(errorSpy.mock.calls);
    expect(logged).toContain('502');
    expect(logged).not.toContain(userSecret);
  });

  it('handles health-check failures without logging the raw exception', async () => {
    const getSpy = jest.spyOn(axios, 'get');
    const failure = new Error(`health failure ${userSecret}`);
    getSpy.mockRejectedValue(failure);

    await expect(checkHealth()).resolves.toBeUndefined();

    const logged = JSON.stringify(errorSpy.mock.calls);
    expect(logged).toContain('Health check');
    expect(logged).not.toContain(userSecret);
  });
});