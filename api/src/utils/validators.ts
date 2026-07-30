import { StrKey } from '@stellar/stellar-sdk';
import { z } from 'zod';

const I128_MIN = -(1n << 127n);
const I128_MAX = (1n << 127n) - 1n;

const parseIntegerString = (value: string) => {
  if (!/^-?\d+$/.test(value)) {
    return undefined;
  }

  try {
    return BigInt(value);
  } catch {
    return undefined;
  }
};

export const StellarAddress = z
  .string()
  .trim()
  .refine(
    value =>
      StrKey.isValidEd25519PublicKey(value) || StrKey.isValidContract(value),
    'Must be a valid Stellar account or contract address'
  );

export const I128String = z
  .string()
  .trim()
  .refine(value => parseIntegerString(value) !== undefined, 'Must be an integer string')
  .refine(value => {
    const amount = parseIntegerString(value);
    return amount !== undefined && amount >= I128_MIN && amount <= I128_MAX;
  }, 'Must fit within signed 128-bit integer range');

export const PositiveI128String = I128String.refine(
  value => {
    const amount = parseIntegerString(value);
    return amount !== undefined && amount > 0n;
  },
  'Amount must be greater than zero'
);
