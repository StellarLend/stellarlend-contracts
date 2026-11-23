import { Router } from 'express';
import * as lendingController from '../controllers/lending.controller';
import {
  depositValidation,
  borrowValidation,
  repayValidation,
  withdrawValidation,
  submitValidation,
} from '../middleware/validation';
import { authenticateToken, verifyHookHmac } from '../middleware/auth';

const router = Router();

router.use('/hooks', verifyHookHmac);
router.post('/hooks', lendingController.processHook);
router.post('/hooks/*', lendingController.processHook);

router.get('/activity', lendingController.getActivity);

router.post('/deposit', depositValidation, lendingController.deposit);
router.post('/borrow', borrowValidation, lendingController.borrow);
router.post('/repay', repayValidation, lendingController.repay);
router.post('/withdraw', withdrawValidation, lendingController.withdraw);
router.post('/submit', authenticateToken, submitValidation, lendingController.submit);

export default router;
