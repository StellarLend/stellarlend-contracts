import { Router } from 'express';
import * as lendingController from '../controllers/lending.controller';

const router = Router();

router.get('/', lendingController.healthCheck);
router.get('/healthz', lendingController.deepHealthCheck);

export default router;
