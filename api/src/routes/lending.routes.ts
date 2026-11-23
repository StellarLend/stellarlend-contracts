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

router.get('/activity', lendingController.getActivity);

router.post('/deposit', depositValidation, lendingController.deposit);
router.post('/borrow', borrowValidation, lendingController.borrow);
router.post('/repay', repayValidation, lendingController.repay);
router.post('/withdraw', withdrawValidation, lendingController.withdraw);

export default router;
