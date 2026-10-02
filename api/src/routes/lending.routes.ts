/**
 * Lending routes invariants
 *
 * POST /hooks and POST /hooks/*
 *   Guarded by `verifyHookHmac` (HMAC-SHA256 + timestamp within 5 min).
 *   No JWT auth required. Any request without a valid HMAC is rejected with 401.
 *
 * POST /deposit
 *   Protected by `depositValidation` (lendingRequestSchema).
 *   Invariants:
 *     - userAddress: valid Stellar G-address or contract (C-address)
 *     - amount: positive integer string within i128 range (> 0, ≤ 2^127 - 1)
 *     - assetAddress: optional; when present must be a valid Stellar address;
 *       empty string is treated as absent
 *     - userSecret: non-empty string (whitespace trimmed)
 *   Validation failures produce HTTP 400 via errorHandler.
 *
 * POST /borrow — same invariants as /deposit
 * POST /repay  — same invariants as /deposit
 * POST /withdraw — same invariants as /deposit
 *
 * Global invariants:
 *   - Validation middleware runs before any controller; controllers never
 *     receive invalid bodies.
 *   - Unhandled errors bubble to errorHandler; routes do not swallow errors.
 *   - Authentication/HMAC errors produce { success: false, error: string }
 *     with no sensitive data in the response.
 */

import { Router } from 'express';
import * as lendingController from '../controllers/lending.controller';
import {
  depositValidation,
  borrowValidation,
  repayValidation,
  withdrawValidation,
} from '../middleware/validation';
import { verifyHookHmac } from '../middleware/auth';

const router = Router();

router.use('/hooks', verifyHookHmac);
router.post('/hooks', lendingController.processHook);
router.post('/hooks/*', lendingController.processHook);

router.post('/deposit', depositValidation, lendingController.deposit);
router.post('/borrow', borrowValidation, lendingController.borrow);
router.post('/repay', repayValidation, lendingController.repay);
router.post('/withdraw', withdrawValidation, lendingController.withdraw);

export default router;
