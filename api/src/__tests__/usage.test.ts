import axios from 'axios';
import {
  borrowAssets,
  checkHealth,
  completeLendingCycle,
  depositCollateral,
  repayDebt,
  withdrawCollateral,
} from '../../examples/usage';

describe('API usage examples', () => {
  let getSpy: jest.SpyInstance;
  let postSpy: jest.SpyInstance;
  let consoleErrorSpy: jest.SpyInstance;
  let consoleLogSpy: jest.SpyInstance;

  beforeEach(() => {
    getSpy = jest.spyOn(axios, 'get').mockResolvedValue({ data: { status: 'healthy' } });
    postSpy = jest.spyOn(axios, 'post').mockResolvedValue({
      data: { success: true, status: 'success', transactionHash: 'tx-1', ledger: 1 },
    });
    consoleErrorSpy = jest.spyOn(console, 'error').mockImplementation(() => undefined);
    consoleLogSpy = jest.spyOn(console, 'log').mockImplementation(() => undefined);
  });

  afterEach(() => {
    jest.restoreAllMocks();
  });

  it.each([
    ['deposit', depositCollateral],
    ['borrow', borrowAssets],
    ['repay', repayDebt],
    ['withdraw', withdrawCollateral],
  ])('sends a valid %s request and returns its response', async (_operation, request) => {
    const expected = { success: true, status: 'success' as const, transactionHash: 'tx-1', ledger: 1 };
    postSpy.mockResolvedValueOnce({ data: expected });

    await expect(request('G-user', '1', 'S-secret')).resolves.toEqual(expected);
    expect(postSpy).toHaveBeenCalledTimes(1);
    expect(postSpy).toHaveBeenCalledWith(
      expect.stringMatching(/\/lending\//),
      expect.objectContaining({ userAddress: 'G-user', amount: '1', userSecret: 'S-secret' })
    );
  });

  it.each(['', '0', '-1', '1.5', '1e3', ' 1'])('rejects invalid amount %j without a request', async (amount) => {
    await expect(depositCollateral('G-user', amount, 'S-secret')).rejects.toThrow(
      'Amount must be a positive whole number of stroops'
    );
    expect(postSpy).not.toHaveBeenCalled();
  });

  it('accepts the maximum supported amount without numeric coercion', async () => {
    const amount = '9223372036854775807';

    await depositCollateral('G-user', amount, 'S-secret');

    expect(postSpy).toHaveBeenCalledWith(
      expect.any(String),
      expect.objectContaining({ amount })
    );
  });

  it('rejects amounts above the supported maximum', async () => {
    await expect(depositCollateral('G-user', '9223372036854775808', 'S-secret')).rejects.toThrow(
      'Amount exceeds the maximum allowed stroops'
    );
    expect(postSpy).not.toHaveBeenCalled();
  });

  it.each([
    ['', '1', 'S-secret'],
    ['G-user', '1', ''],
  ])('rejects missing required transaction values before sending', async (address, amount, secret) => {
    await expect(depositCollateral(address, amount, secret)).rejects.toThrow();
    expect(postSpy).not.toHaveBeenCalled();
  });

  it('reports unhealthy status as a failure', async () => {
    getSpy.mockResolvedValueOnce({ data: { status: 'degraded', services: { database: 'down' } } });

    await expect(checkHealth()).rejects.toThrow('API health check failed');
    expect(consoleErrorSpy).toHaveBeenCalled();
  });

  it('aborts the lending cycle after a failed transaction without advancing state', async () => {
    postSpy.mockResolvedValueOnce({ data: { success: false, status: 'failed', error: 'private detail' } });

    await expect(completeLendingCycle()).rejects.toThrow('Deposit did not complete');
    expect(getSpy).toHaveBeenCalledTimes(1);
    expect(postSpy).toHaveBeenCalledTimes(1);
    expect(consoleLogSpy).not.toHaveBeenCalledWith(expect.stringContaining('private detail'));
  });

  it('does not advance while a transaction is still pending', async () => {
    postSpy.mockResolvedValueOnce({
      data: { success: true, status: 'pending', transactionHash: 'tx-pending' },
    });

    await expect(completeLendingCycle()).rejects.toThrow('Deposit did not complete');
    expect(postSpy).toHaveBeenCalledTimes(1);
  });

  it('does not retry a rejected write', async () => {
    postSpy.mockRejectedValueOnce(new Error('network unavailable'));

    await expect(depositCollateral('G-user', '1', 'S-secret')).rejects.toThrow('network unavailable');
    expect(postSpy).toHaveBeenCalledTimes(1);
    expect(consoleErrorSpy).toHaveBeenCalledWith('❌ Deposit failed');
  });

  it('keeps concurrent requests independent', async () => {
    postSpy
      .mockResolvedValueOnce({ data: { success: true, status: 'success', transactionHash: 'tx-a' } })
      .mockResolvedValueOnce({ data: { success: true, status: 'success', transactionHash: 'tx-b' } });

    const results = await Promise.all([
      depositCollateral('G-a', '1', 'S-a'),
      depositCollateral('G-b', '2', 'S-b'),
    ]);

    expect(results.map(({ transactionHash }) => transactionHash)).toEqual(['tx-a', 'tx-b']);
    expect(postSpy).toHaveBeenCalledTimes(2);
    expect(postSpy).toHaveBeenNthCalledWith(
      1,
      expect.any(String),
      expect.objectContaining({ userAddress: 'G-a', amount: '1', userSecret: 'S-a' })
    );
    expect(postSpy).toHaveBeenNthCalledWith(
      2,
      expect.any(String),
      expect.objectContaining({ userAddress: 'G-b', amount: '2', userSecret: 'S-b' })
    );
  });

  it('logs HTTP status without exposing API error payloads', async () => {
    const axiosError = Object.assign(new Error('request failed'), {
      isAxiosError: true,
      response: { status: 400, data: { error: 'S-sensitive-value' } },
    });
    jest.spyOn(axios, 'isAxiosError').mockReturnValue(true);
    postSpy.mockRejectedValueOnce(axiosError);

    await expect(depositCollateral('G-user', '1', 'S-secret')).rejects.toBe(axiosError);

    expect(consoleErrorSpy).toHaveBeenCalledWith('❌ Deposit failed with HTTP 400');
    expect(consoleErrorSpy).not.toHaveBeenCalledWith(expect.stringContaining('S-sensitive-value'));
  });
});