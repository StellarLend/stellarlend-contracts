import { Router, Request, Response, NextFunction } from 'express';
import * as lendingController from '../controllers/lending.controller';
import logger from '../utils/logger';

/**
 * Health routes are public by design (load balancers, orchestrators,
 * monitoring probes). They must not require authentication, but they
 * must be deterministic and must not leak internal details on failure.
 *
 * Invariants:
*  1. GET health endpoints are idempotent and read-only.
 *  2. Only GET and HEAD are allowed; any other method returns 405.
 *  3. Unknown subpaths return 404 with a stable JSON shape.
*  4. Errors from downstream controllers are normalized to 503 with
 *     a non-sensitive payload and logged with request context.
 *  5. No query parameters are required or trusted; unknown query keys are
 *     ignored so clients in different versions remain compatible.
 */

const router = Router();

const ALLOWED_METHODS = ['GET', 'HEAD'] as const;

const methodNotAllowed = (req: Request, res: Response): void => {
  res.set('Allow', ALLOWED_METHODS.join(', '));
  res.status(405).json({
    status: 'error',
    code: 'method_not_allowed',
    message: 'Method not allowed on health endpoints',
  });
};

const notFound = (req: Request, res: Response): void => {
  res.status(404).json({
    status: 'error',
    code: 'health_route_not_found',
    message: 'Health endpoint not found',
  });
};

/**
 * Wraps a controller handler so that any synchronous or asynchronous
 * failure is normalized into a 503 with a stable shape. This prevents
 * internal error messages (or stack traces) from being returned to clients
 * and ensures monitoring probes get a deterministic response.
 */
function safeHandler(
  name: string,
  handler: (req: Request, res: Response, next: NextFunction) => unknown,
) : (req: Request, res: Response, next: NextFunction) => void {
  return (req, res, next): void => {
    try {
      const result = handler(req, res, next);
      if (result && typeof (result as Promise<unknown>).then === 'function') {
        (result as Promise<unknown>).catch((err) => {
          logger.error('health route handler failed', {
            err,
            route: name,
            method: req.method,
            path: req.originalUrl,
          });
          if (!res.headersSent) {
            res.status(503).json({
              status: 'error',
              code: 'health_unavailable',
              message: 'Health check temporarily unavailable',
            });
          }
        });
      }
    } catch (err) {
      logger.error('health route handler threw', {
        err,
        route: name,
        method: req.method,
        path: req.originalUrl,
      });
      if (!res.headersSent) {
        res.status(503).json({
          status: 'error',
          code: 'health_unavailable',
          message: 'Health check temporarily unavailable',
        });
      }
    }
  };
}

/**
 * Reject any non-GET/HEAD method before the controller is invoked.
 * This keeps the health surface read-only and avoids accidental mutations
 * from being attempted against health endpoints.
 */
router.all('/', (req, res, next) => {
  if (!ALLOWED_METHODS.includes(req.method as (typeof ALLOWED_METHODS)[number])) {
    return methodNotAllowed(req, res);
  }
  return next();
});

router.all('/healthz', (req, res, next) => {
  if (!ALLOWED_METHODS.includes(req.method as (typeof ALLOWED_METHODS)[number])) {
    return methodNotAllowed(req, res);
  }
  return next();
});

router.get('/', safeHandler('health', lendingController.healthCheck));
router.get('/healthz', safeHandler('deepHealth', lendingController.deepHealthCheck));

// Any unknown subpath must return a deterministic 404 instead of falling
// through to the global error handler or leaking route information.
router.use(notFound);

export default router;
