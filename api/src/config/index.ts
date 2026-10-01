import dotenv from 'dotenv';

dotenv.config();

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Parse an integer environment variable.
 * Returns `defaultValue` when the variable is absent or blank.
 * Throws a descriptive error when the value is present but non-numeric.
 */
function parseIntEnv(name: string, defaultValue: number): number {
  const raw = process.env[name];
  if (raw === undefined || raw.trim() === '') return defaultValue;
  const num = Number(raw);
  if (!Number.isInteger(num)) {
    throw new Error(`Config: environment variable ${name} must be an integer, got "${raw}"`);
  }
  return num;
}

/**
 * Parse a float environment variable.
 * Returns `defaultValue` when the variable is absent or blank.
 * Throws a descriptive error when the value is present but non-numeric.
 */
function parseFloatEnv(name: string, defaultValue: number): number {
  const raw = process.env[name];
  if (raw === undefined || raw.trim() === '') return defaultValue;
  const num = Number(raw);
  if (!Number.isFinite(num)) {
    throw new Error(`Config: environment variable ${name} must be a number, got "${raw}"`);
  }
  return num;
}

/**
 * Read a string environment variable.
 * Returns `defaultValue` when the variable is absent or blank.
 */
function stringEnv(name: string, defaultValue: string): string {
  const raw = process.env[name];
  return raw !== undefined && raw.trim() !== '' ? raw.trim() : defaultValue;
}

// ---------------------------------------------------------------------------
// Known-insecure sentinel values
// ---------------------------------------------------------------------------

const INSECURE_JWT_DEFAULTS = new Set([
  'default-secret-change-me',
  'your-secret-key-change-in-production',
  'secret',
  'changeme',
  '',
]);

const INSECURE_HOOK_DEFAULTS = new Set([
  'your-hook-secret-change-in-production',
  'changeme',
  '',
]);

const VALID_NETWORKS = ['testnet', 'public', 'futurenet'] as const;
type StellarNetwork = (typeof VALID_NETWORKS)[number];

const VALID_LOG_LEVELS = ['error', 'warn', 'info', 'http', 'verbose', 'debug', 'silly'] as const;
type LogLevel = (typeof VALID_LOG_LEVELS)[number];

const VALID_NODE_ENVS = ['development', 'test', 'production', 'staging'] as const;

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/**
 * Invariant checks that must hold at start-up regardless of environment.
 * Throws early with an actionable message rather than failing later at runtime.
 */
function validateConfig(cfg: typeof config): void {
  // --- server ---
  if (cfg.server.port < 1 || cfg.server.port > 65535) {
    throw new Error(
      `Config: PORT must be between 1 and 65535, got ${cfg.server.port}`
    );
  }

  if (!VALID_NODE_ENVS.includes(cfg.server.env as (typeof VALID_NODE_ENVS)[number])) {
    throw new Error(
      `Config: NODE_ENV must be one of [${VALID_NODE_ENVS.join(', ')}], got "${cfg.server.env}"`
    );
  }

  // --- stellar ---
  if (!VALID_NETWORKS.includes(cfg.stellar.network as StellarNetwork)) {
    throw new Error(
      `Config: STELLAR_NETWORK must be one of [${VALID_NETWORKS.join(', ')}], got "${cfg.stellar.network}"`
    );
  }

  if (!cfg.stellar.horizonUrl.startsWith('http')) {
    throw new Error(
      `Config: HORIZON_URL must be a valid URL, got "${cfg.stellar.horizonUrl}"`
    );
  }

  if (!cfg.stellar.sorobanRpcUrl.startsWith('http')) {
    throw new Error(
      `Config: SOROBAN_RPC_URL must be a valid URL, got "${cfg.stellar.sorobanRpcUrl}"`
    );
  }

  if (!cfg.stellar.networkPassphrase || cfg.stellar.networkPassphrase.trim() === '') {
    throw new Error('Config: NETWORK_PASSPHRASE must not be empty');
  }

  // --- auth ---
  if (cfg.auth.jwtSecret.trim() === '') {
    throw new Error('Config: JWT_SECRET must not be empty');
  }

  if (!cfg.auth.jwtExpiresIn || cfg.auth.jwtExpiresIn.trim() === '') {
    throw new Error('Config: JWT_EXPIRES_IN must not be empty');
  }

  // --- rateLimit ---
  if (cfg.rateLimit.windowMs < 1) {
    throw new Error(
      `Config: RATE_LIMIT_WINDOW_MS must be a positive number, got ${cfg.rateLimit.windowMs}`
    );
  }

  if (cfg.rateLimit.maxRequests < 1) {
    throw new Error(
      `Config: RATE_LIMIT_MAX_REQUESTS must be at least 1, got ${cfg.rateLimit.maxRequests}`
    );
  }

  // --- logging ---
  if (!VALID_LOG_LEVELS.includes(cfg.logging.level as LogLevel)) {
    throw new Error(
      `Config: LOG_LEVEL must be one of [${VALID_LOG_LEVELS.join(', ')}], got "${cfg.logging.level}"`
    );
  }

  // --- request ---
  if (cfg.request.timeout < 1) {
    throw new Error(
      `Config: REQUEST_TIMEOUT must be a positive number, got ${cfg.request.timeout}`
    );
  }

  if (cfg.request.maxRetries < 0) {
    throw new Error(
      `Config: MAX_RETRIES must be >= 0, got ${cfg.request.maxRetries}`
    );
  }

  if (cfg.request.retryInitialDelayMs < 1) {
    throw new Error(
      `Config: RETRY_INITIAL_DELAY_MS must be a positive number, got ${cfg.request.retryInitialDelayMs}`
    );
  }

  if (cfg.request.retryMaxDelayMs < cfg.request.retryInitialDelayMs) {
    throw new Error(
      `Config: RETRY_MAX_DELAY_MS (${cfg.request.retryMaxDelayMs}) must be >= RETRY_INITIAL_DELAY_MS (${cfg.request.retryInitialDelayMs})`
    );
  }

  // --- circuitBreaker ---
  if (cfg.circuitBreaker.windowMs < 1) {
    throw new Error(
      `Config: CB_WINDOW_MS must be a positive number, got ${cfg.circuitBreaker.windowMs}`
    );
  }

  if (cfg.circuitBreaker.failureThreshold <= 0 || cfg.circuitBreaker.failureThreshold > 1) {
    throw new Error(
      `Config: CB_FAILURE_THRESHOLD must be in (0, 1], got ${cfg.circuitBreaker.failureThreshold}`
    );
  }

  if (cfg.circuitBreaker.minRequests < 1) {
    throw new Error(
      `Config: CB_MIN_REQUESTS must be at least 1, got ${cfg.circuitBreaker.minRequests}`
    );
  }

  if (cfg.circuitBreaker.openMs < 1) {
    throw new Error(
      `Config: CB_OPEN_MS must be a positive number, got ${cfg.circuitBreaker.openMs}`
    );
  }

  if (cfg.circuitBreaker.halfOpenMaxTrial < 1) {
    throw new Error(
      `Config: CB_HALF_OPEN_TRIAL must be at least 1, got ${cfg.circuitBreaker.halfOpenMaxTrial}`
    );
  }
}

/**
 * Emit console warnings for insecure defaults that are acceptable in
 * development but must be replaced before going to production.
 * Throws in production to prevent unsafe deployments.
 */
function auditSecurityDefaults(cfg: typeof config): void {
  const isProduction = cfg.server.env === 'production';

  if (INSECURE_JWT_DEFAULTS.has(cfg.auth.jwtSecret)) {
    const msg =
      'Config: JWT_SECRET is set to an insecure default value. ' +
      'Set a strong, unique secret via the JWT_SECRET environment variable.';
    if (isProduction) {
      throw new Error(msg);
    }
    console.warn(`[SECURITY WARNING] ${msg}`);
  }

  if (cfg.auth.hookSecret && INSECURE_HOOK_DEFAULTS.has(cfg.auth.hookSecret)) {
    const msg =
      'Config: STELLAR_API_HOOK_SECRET is set to an insecure default value. ' +
      'Set a strong, unique secret via the STELLAR_API_HOOK_SECRET environment variable.';
    if (isProduction) {
      throw new Error(msg);
    }
    console.warn(`[SECURITY WARNING] ${msg}`);
  }

  if (isProduction && cfg.stellar.contractId.trim() === '') {
    throw new Error(
      'Config: CONTRACT_ID must be set in production. ' +
      'Provide it via the CONTRACT_ID environment variable.'
    );
  }
}

// ---------------------------------------------------------------------------
// Config object
// ---------------------------------------------------------------------------

export const config = {
  server: {
    port: parseIntEnv('PORT', 3000),
    env: stringEnv('NODE_ENV', 'development'),
  },
  stellar: {
    network: stringEnv('STELLAR_NETWORK', 'testnet'),
    horizonUrl: stringEnv('HORIZON_URL', 'https://horizon-testnet.stellar.org'),
    sorobanRpcUrl: stringEnv('SOROBAN_RPC_URL', 'https://soroban-testnet.stellar.org'),
    networkPassphrase: stringEnv('NETWORK_PASSPHRASE', 'Test SDF Network ; September 2015'),
    contractId: stringEnv('CONTRACT_ID', ''),
  },
  auth: {
    jwtSecret: stringEnv('JWT_SECRET', 'default-secret-change-me'),
    jwtExpiresIn: stringEnv('JWT_EXPIRES_IN', '24h'),
    hookSecret: stringEnv('STELLAR_API_HOOK_SECRET', ''),
  },
  rateLimit: {
    windowMs: parseIntEnv('RATE_LIMIT_WINDOW_MS', 900000),
    maxRequests: parseIntEnv('RATE_LIMIT_MAX_REQUESTS', 100),
  },
  logging: {
    level: stringEnv('LOG_LEVEL', 'info'),
  },
  request: {
    timeout: parseIntEnv('REQUEST_TIMEOUT', 30000),
    maxRetries: parseIntEnv('MAX_RETRIES', 3),
    retryInitialDelayMs: parseIntEnv('RETRY_INITIAL_DELAY_MS', 1000),
    retryMaxDelayMs: parseIntEnv('RETRY_MAX_DELAY_MS', 10000),
  },
  circuitBreaker: {
    windowMs: parseIntEnv('CB_WINDOW_MS', 60000),
    failureThreshold: parseFloatEnv('CB_FAILURE_THRESHOLD', 0.5),
    minRequests: parseIntEnv('CB_MIN_REQUESTS', 5),
    openMs: parseIntEnv('CB_OPEN_MS', 30000),
    halfOpenMaxTrial: parseIntEnv('CB_HALF_OPEN_TRIAL', 2),
  },
};

// Run validation at module load time so misconfiguration is caught immediately.
validateConfig(config);
auditSecurityDefaults(config);

// ---------------------------------------------------------------------------
// Exported helpers for testability
// ---------------------------------------------------------------------------

export { validateConfig, auditSecurityDefaults, parseIntEnv, parseFloatEnv, stringEnv };
export type { StellarNetwork, LogLevel };
