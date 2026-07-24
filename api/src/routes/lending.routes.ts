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
