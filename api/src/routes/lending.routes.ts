import { Router } from 'express';
import * as lendingController from '../controllers/lending.controller';
import {
  depositValidation,
  borrowValidation,
  repayValidation,
  withdrawValidation,
} from '../middleware/validation';
import {
  authenticateToken,
  requireAuthenticatedWallet,
  requireSameAddress,
  generateToken,
  verifyHookHmac,
} from '../middleware/authorization';

const router = Router();

router.use('/hooks', verifyHookHmac);
router.post('/hooks', lendingController.processHook);
router.post('/hooks/*', lendingController.processHook);

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
