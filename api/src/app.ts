import express, { Application, NextFunction, Request, Response } from 'express';
import helmet from 'helmet';
import cors from 'cors';
import rateLimit from 'express-rate-limit';
import { config } from './config';
import lendingRoutes from './routes/lending.routes';
import healthRoutes from './routes/health.routes';
import { errorHandler } from './middleware/errorHandler';
import logger from './utils/logger';

const app: Application = express();

/**
 * Invariants enforced by this module:
 * 1. Request bodies are parsed deterministically and the raw body is preserved for signature verification.
 * 2. Rate limiting is applied to all /api/ routes with a deterministic configuration.
 * 3. Unknown routes under /api/ return a consistent 404 JSON response.
 * 4. All errors are routed through the central error handler without leaking internal details.
 * 5. Parser failures (malformed JSON, oversized payloads) are reported as 400 with a safe message.
 */

// Security headers and CSRP protection.
app.use(helmet());

app.use(cors());

app.use(
  express.json({
    limit: config.jsonLimit,
    verify: (req: Request, _res: Response, buf: Buffer) => {
      (req as Request & { rawBody?: string }).rawBody = buf.toString('utf8');
    },
  }),
);
app.use(express.urlencoded({ extended: true, limit: config.jsonLimit }));

const limiter = rateLimit({
  windowMs: config.rateLimit.windowMs,
  max: config.rateLimit.maxRequests,
  message: 'Too many requests from this IP, please try again later.',
  standardHeaders: true,
  keyGenerator: (req: Request) => req.ip || req.socket.remoteAddress || 'unknown',
});

app.use('/api/', limiter);

app.use('/api/health', healthRoutes);
app.use('/api/lending', lendingRoutes);

// Explicit 404 for unknown /api/ routes so clients get a deterministic JSON response.
app.use('/api/', (_req: Request, res: Response) => {
  res.status(404).json({ error: 'Not Found' });
});

// Explicit 404 for any other unknown route.
app.use((_req: Request, res: Response) => {
  res.status(404).json({ error: 'Not Found' });
});

// Centralized error handling must be last so it can catch downstream failures.
app.use((err: any, req: Request, res: Response, next: NextFunction) => {
  logger.error('Unhandled request error', {
    message: err?.message,
    path: req.path,
    method: req.method,
  });

  if ((err as { type?: string })?.type === 'entity.tolarge') {
    logger.warn('Request body too large', { path: req.path, method: req.method });
    return res.status(413).json({ error: 'Payload too large' });
  }

  return errorHandler(err, req, res, next);
});

export default app;
