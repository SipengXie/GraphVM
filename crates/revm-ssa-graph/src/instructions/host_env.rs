use revm_primitives::db::DatabaseRef;
use revm_primitives::{Spec, U256};
use revm_ssa::{SSAInput, SSALogEntry, SSAOutput};
use crate::{ExecutionContext, ExecutionError, Result, get_ssa_output_stack_or_const, SsaGraph};
use crate::as_usize_saturated;

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Execute host environment operation
    #[inline(always)]
    pub fn execute_host_env(&self, node: &mut SSALogEntry, _graph: & SsaGraph, opcode: u8) -> Result<()> {
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
        node.outputs[0] = SSAOutput::Stack(value);
        Ok(())
    }

    /// Execute BLOBHASH operation
    #[inline(always)]
    pub fn execute_blobhash(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let value = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let index = as_usize_saturated!(value);
        let tx = &self.env().tx;
        let value = match tx.blob_hashes.get(index) {
            Some(hash) => U256::from_be_bytes(hash.0),
            None => U256::ZERO,
        };
        node.outputs[0] = SSAOutput::Stack(value);
        Ok(())
    }
}