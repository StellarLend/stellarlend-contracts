import app from './app';
import { config } from './config';
import logger from './utils/logger';

const PORT = config.server.port;

app.listen(PORT, () => {
  logger.info(`StellarLend API server running on port ${PORT}`);
  logger.info(`Environment: ${config.server.env}`);
  logger.info(`Network: ${config.stellar.network}`);
});

process.on('unhandledRejection', (reason, promise) => {
  logger.error('Unhandled Rejection at:', promise, 'reason:', reason);
  process.exit(1);
});

process.on('uncaughtException', (error) => {
  logger.error('Uncaught Exception:', error);
  process.exit(1);
});
