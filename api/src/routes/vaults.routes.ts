import { Router } from 'express';
import * as vaultController from '../controllers/vault.controller';
import { createVaultSchema, updateVaultSchema } from '../services/vaultValidation';
import { validateRequest } from '../middleware/validation';

const router = Router();

router.post('/vaults', createVaultSchema, validateRequest, vaultController.createVault);
router.get('/vaults', vaultController.getVaults);
router.get('/vaults/:id', vaultController.getVault);
router.put('/vaults/:id', updateVaultSchema, validateRequest, vaultController.updateVault);
router.delete('/vaults/:id', vaultController.deleteVault);

export default router;
