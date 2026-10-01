import express, { Application } from 'express';
import helmet from 'helmet';
import cors from 'cors';
import rateLimit from 'express-rate-limit';
import { config } from './config';
import lendingRoutes from './routes/lending.routes';
import healthRoutes from './routes/health.routes';
import { errorHandler } from './middleware/errorHandler';
import logger from './utils/logger';

const app: Application = express();

// Security headers and CSRP protection.
app.use(helmet());

app.use(cors());

app.use(express.json({
  verify: (req, res, buf) => {
    (req as any).rawBody = buf.toString('utf8');
  },
}));
app.use(express.urlencoded({ extended: true }));

const limiter = rateLimit({
  windowMs: config.rateLimit.windowMs,
  max: config.rateLimit.maxRequests,
  message: 'Too many requests from this IP, please try again later.',
});

app.use('/api/', limiter);

app.use('/api/health', healthRoutes);
app.use('/api/lending', lendingRoutes);

// Centralized error handling must be last so it can catch downstream failures.
app.use((err, req, res, next) => {
  logger.error('Unhandled request error', {
    message: err?.message,
    path: req.path,
    method: req.method,
  });
  errorHandler(err, req, res, next);
});

export default app;
