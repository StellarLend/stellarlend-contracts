import express, { Application, NextFunction, Request, Response } from 'express';
import helmet from 'helmet';
import cors from 'cors';
import rateLimit from 'express-rate-limit';
import { config } from './config';
import lendingRoutes from './routes/lending.routes';
import healthRoutes from './routes/health.routes';
import { errorHandler } from './middleware/errorHandler';

const app: Application = express();

/**
 * Invariants enforced by this module:
 * 1. Request bodies are parsed deterministically and the raw body is preserved for signature verification.
 * 2. Rate limiting is applied to all /api/ routes with a deterministic configuration.
 * 3. Unknown routes under /api/ return a consistent 404 JSON response.
 * 4. All errors are routed through the central error handler without leaking internal details.
 * 5. Parser failures are reported as 400 (malformed JSON) or 413 (oversized payload) with a
 *    safe message.
 */

// Security headers and CSRP protection.
app.use(helmet());

// Vary rejected and accepted responses by Origin so shared caches cannot
// reuse a response across different browser origins.
app.use((_req: Request, res: Response, next: NextFunction) => {
  res.vary('Origin');
  next();
});

app.use(cors({
  origin: (requestOrigin, callback) => {
    if (!requestOrigin) {
      callback(null, false);
      return;
    }

    const allowed = config.cors.origins;
    callback(null, allowed.includes('*') || allowed.includes(requestOrigin) ? requestOrigin : false);
  },
}));

app.use(
  express.json({
    limit: config.server.bodyLimit,
    verify: (req: Request, _res: Response, buf: Buffer) => {
      (req as Request & { rawBody?: string }).rawBody = buf.toString('utf8');
    },
  }),
);

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
  return errorHandler(err, req, res, next);
});

export default app;
