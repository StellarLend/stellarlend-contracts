import { createHmac, timingSafeEqual } from 'crypto';
import { IncomingHttpHeaders } from 'http';
import { UnauthorizedError } from './errors';

export enum HookHeader {
  Signature = 'x-hook-signature',
  Timestamp = 'x-hook-timestamp',
}

export const HOOK_WINDOW_MS = 5 * 60 * 1000;
export const HOOK_HMAC_ALGORITHM = 'sha256';
export const HOOK_SIGNATURE_ENCODING = 'hex' as const;
export const HOOK_SIGNATURE_PATTERN = /^[0-9a-f]{64}$/i;

export interface HookAuthMessages {
  secretNotConfigured: string;
  missingHeaders: string;
  invalidTimestamp: string;
  timestampOutsideWindow: string;
  missingRawBody: string;
  invalidSignature: string;
}

export const DEFAULT_HOOK_AUTH_MESSAGES: HookAuthMessages = {
  secretNotConfigured: 'Hook authentication secret is not configured',
  missingHeaders: 'Hook signature and timestamp headers are required',
  invalidTimestamp: 'Invalid hook timestamp',
  timestampOutsideWindow: 'Hook timestamp outside allowable window',
  missingRawBody: 'Raw request body is required for hook authentication',
  invalidSignature: 'Invalid hook signature',
};

export interface AssertHookHmacInput {
  headers: IncomingHttpHeaders;
  rawBody?: string;
  secret?: string;
  now?: number;
  messages?: Partial<HookAuthMessages>;
}

const firstHeader = (value: string | string[] | undefined): string | undefined => {
  if (Array.isArray(value)) {
    return value[0];
  }

  return value;
};

const fail = (message: string): never => {
  throw new UnauthorizedError(message);
};

export const assertHookHmac = (input: AssertHookHmacInput): void => {
  const messages = { ...DEFAULT_HOOK_AUTH_MESSAGES, ...input.messages };
  const signature = firstHeader(input.headers[HookHeader.Signature]);
  const timestampValue = firstHeader(input.headers[HookHeader.Timestamp]);

  if (!input.secret) {
    fail(messages.secretNotConfigured);
  }

  if (!signature || !timestampValue) {
    fail(messages.missingHeaders);
  }

  const timestamp = Number(timestampValue);

  if (!Number.isFinite(timestamp)) {
    fail(messages.invalidTimestamp);
  }

  const now = input.now ?? Date.now();

  if (Math.abs(now - timestamp) > HOOK_WINDOW_MS) {
    fail(messages.timestampOutsideWindow);
  }

  if (typeof input.rawBody !== 'string') {
    fail(messages.missingRawBody);
  }

  if (!HOOK_SIGNATURE_PATTERN.test(signature)) {
    fail(messages.invalidSignature);
  }

  const expectedSignature = createHmac(HOOK_HMAC_ALGORITHM, input.secret)
    .update(`${timestampValue}.${input.rawBody}`)
    .digest(HOOK_SIGNATURE_ENCODING);
  const signatureBuffer = Buffer.from(signature, HOOK_SIGNATURE_ENCODING);
  const expectedBuffer = Buffer.from(expectedSignature, HOOK_SIGNATURE_ENCODING);

  if (
    signatureBuffer.length !== expectedBuffer.length ||
    !timingSafeEqual(signatureBuffer, expectedBuffer)
  ) {
    fail(messages.invalidSignature);
  }
};
