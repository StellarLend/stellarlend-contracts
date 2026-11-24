import { Router } from 'express';
import * as lendingController from '../controllers/lending.controller';
import {
  depositValidation,
  borrowValidation,
  repayValidation,
  withdrawValidation,
  submitValidation,
} from '../middleware/validation';
import {
  authenticateToken,
  requireAuthenticatedWallet,
  requireSameAddress,
  generateToken,
  verifyHookHmac,
} from '../middleware/authorization';

const router = Router();

const SUPPORTED_HOOK_NAMES = new Set(['indexer']);

router.use('/hooks', verifyHookHmac);
router.post('/hooks', lendingController.processHook);
router.post('/hooks/:name', (req, res, next) => {
  if (!SUPPORTED_HOOK_NAMES.has(req.params.name)) {
    res.status(404).json({ error: 'Not Found' });
    return;
  }

  lendingController.processHook(req, res, next);
});

router.post('/token', (req, res) => {
  const { address, network } = req.body ?? {};
  if (!address || typeof address !== 'string' || !address.startsWith('G') || address.length !== 56) {
    res.status(400).json({ error: 'Valid Stellar address required' });
    return;
  }
  const token = generateToken(address, network);
  res.json({ token });
});

router.get('/activity', lendingController.getActivity);

router.post(
  '/deposit',
  authenticateToken,
  requireAuthenticatedWallet,
  depositValidation,
  requireSameAddress,
  lendingController.deposit
);
router.post(
  '/borrow',
  authenticateToken,
  requireAuthenticatedWallet,
  borrowValidation,
  requireSameAddress,
  lendingController.borrow
);
router.post(
  '/repay',
  authenticateToken,
  requireAuthenticatedWallet,
  repayValidation,
  requireSameAddress,
  lendingController.repay
);
router.post(
  '/withdraw',
  authenticateToken,
  requireAuthenticatedWallet,
  withdrawValidation,
  requireSameAddress,
  lendingController.withdraw
);

export default router;
