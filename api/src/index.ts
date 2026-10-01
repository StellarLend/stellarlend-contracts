import { Server } from 'http';
import app from './app';
import { config } from './config';
import logger from './utils/logger';

const PORT = config.server.port;

/**
 * Start the HTTP server and return the Server instance.
 *
 * Invariants:
 *  - The server listens on exactly one port per process.
 *  - Calling startServer twice in the same process is a bug; the caller must
 *    stop the first server before starting another.
 *  - The returned server is the only handle that owns the listening socket;
 *     shutdown must go through this handle.
 */
export const startServer = (): Server => {
  const server = app.listen(PORT, () => {
    logger.info(`StellarLend API server running on port ${PORT}`);
    logger.info(`Environment: ${config.server.env}`);
    logger.info(`Network: ${config.stellar.network}`);
  });

  // Listen errors (E.ADDRINUSE, E.ACCESS, etc.) must not be swallowed.
  // The process exits non-zero so the orchestrator can restart it.
  server.on('error', (error: NodeJS.ErrnoException): void => {
    logger.error('Server failed to listen:', {
      code: error.code,
      message: error.message,
    });
    process.exit(1);
  });

  return server;
};

/**
 * Graceful shutdown helper.
 *
 * Invariants:
 *  - Idempotent: calling it multiple times does not throw.
 *  - Resolves once the server has stopped accepting new connections.
 *  - Never exits the process itself; the caller decides the exit code.
 */
export const stopServer = (server: Server): Promise<void> =>
  new Promise((resolve, reject) => {
    if (!server.listening) {
      resolve();
      return;
    }
    server.close((err) => {
      if (err) {
        reject(err);
        return;
      }
      resolve();
    });
  });

// Only auto-start when executed as the process entry point. This keeps the
// module safe to import from tests and other consumers without side effects.
if (require.main === module) {
  const server = startServer();

  const shutdown = async (signal: string): Promise<void> => {
    logger.info(`Received ${signal}, shutting down gracefully`);
    try {
      await stopServer(server);
      process.exit(0);
    } catch (error) {
      logger.error('Error during graceful shutdown:', error);
      process.exit(1);
    }
  };

  process.on('SIGINT', () => void shutdown('SIGINT'));
  process.on('SIGTERM', () => void shutdown('SIGTERM'));

  process.on('unhandledRejection', (reason, promise) => {
    logger.error('Unhandled Rejection at:', promise, 'reason:', reason);
    process.exit(1);
  });

  process.on('uncaughtException', (error) => {
    logger.error('Uncaught Exception:', error);
    process.exit(1);
  });
}
