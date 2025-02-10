use revm_primitives::db::DatabaseRef;
use revm_primitives::{Spec, U256};
use revm_ssa::{SSAInput, SSAOutput};
use crate::{ExecutionContext, ExecutionError, Result};


use super::as_usize_saturated;

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Execute host environment operation
    #[inline]
    pub fn execute_host_env(&self, inputs: Vec<SSAInput>, opcode: u8) -> Result<Vec<SSAOutput>> {
        if !inputs.is_empty() {
            return Err(ExecutionError::ExecutionError(
                format!("opcode 0x{:x} requires 0 operands", opcode)
            ));
        }

        let value = match opcode {
            // CHAINID
            0x46 => U256::from(self.env().cfg.chain_id),
            // COINBASE
            0x41 => U256::from_be_bytes(self.env().block.coinbase.into_word().into()),
            // TIMESTAMP
            0x42 => self.env().block.timestamp,
            // NUMBER
            0x43 => self.env().block.number,
            // DIFFICULTY/PREVRANDAO
            0x44 => {
                if let Some(prevrandao) = self.env().block.prevrandao {
                    U256::from_be_bytes(prevrandao.0)
                } else {
                    self.env().block.difficulty
                }
            }
            // GASLIMIT
            0x45 => self.env().block.gas_limit,
            // GASPRICE
            0x3a => self.env().effective_gas_price(),
            // BASEFEE
            0x48 => self.env().block.basefee,
            // ORIGIN
            0x32 => U256::from_be_bytes(self.env().tx.caller.into_word().into()),
            // BLOBBASEFEE
            0x4a => U256::from(
                self.env().block.get_blob_gasprice().unwrap_or_default()
            ),
            _ => return Err(ExecutionError::ExecutionError(
                format!("Unknown host environment opcode: 0x{:x}", opcode)
            )),
        };
        Ok(vec![SSAOutput::Stack(value)])
    }

    /// Execute BLOBHASH operation
    #[inline]
    pub fn execute_blobhash(&self, inputs: Vec<SSAInput>, opcode: u8) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                format!("opcode 0x{:x} requires 1 operand", opcode)
            ));
        }
        let index = match inputs[0] {   
            SSAInput::Stack { value, .. } => as_usize_saturated(value),
            _ => return Err(ExecutionError::ExecutionError(
                format!("opcode 0x{:x} requires 1 operand", opcode)
            )),
        };
        let tx = &self.env().tx;
        let value = match tx.blob_hashes.get(index) {
            Some(hash) => U256::from_be_bytes(hash.0),
            None => U256::ZERO,
        };
        Ok(vec![SSAOutput::Stack(value)])
    }
}