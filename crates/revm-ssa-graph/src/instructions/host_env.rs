use crate::as_usize_saturated;
use crate::{get_ssa_output_stack_or_const, ExecutionContext, ExecutionError, Result, SsaGraph};
use revm_primitives::db::DatabaseRef;
use revm_primitives::U256;
use revm_ssa::{SSAInput, SSALogEntry, SSAOutput};

#[inline(always)]
pub fn execute_host_env<DB: DatabaseRef + Sync + Send>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    _graph: &SsaGraph,
    opcode: u8,
) -> Result<()> {
    let value = match opcode {
        // CHAINID
        0x46 => U256::from(_context.env().cfg.chain_id),
        // COINBASE
        0x41 => U256::from_be_bytes(_context.env().block.coinbase.into_word().into()),
        // TIMESTAMP
        0x42 => _context.env().block.timestamp,
        // NUMBER
        0x43 => _context.env().block.number,
        // DIFFICULTY/PREVRANDAO
        0x44 => {
            if let Some(prevrandao) = _context.env().block.prevrandao {
                U256::from_be_bytes(prevrandao.0)
            } else {
                _context.env().block.difficulty
            }
        }
        // GASLIMIT
        0x45 => _context.env().block.gas_limit,
        // GASPRICE
        0x3a => _context.env().effective_gas_price(),
        // BASEFEE
        0x48 => _context.env().block.basefee,
        // ORIGIN
        0x32 => U256::from_be_bytes(_context.env().tx.caller.into_word().into()),
        // BLOBBASEFEE
        0x4a => U256::from(_context.env().block.get_blob_gasprice().unwrap_or_default()),
        _ => {
            return Err(ExecutionError::ExecutionError(format!(
                "Unknown host environment opcode: 0x{:x}",
                opcode
            )))
        }
    };
    node.outputs[0] = SSAOutput::Stack(value);
    Ok(())
}

/// Execute BLOBHASH operation
#[inline(always)]
pub fn execute_blobhash<DB: DatabaseRef + Sync + Send>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let value = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let index = as_usize_saturated!(value);
    let tx = &_context.env().tx;
    let value = match tx.blob_hashes.get(index) {
        Some(hash) => U256::from_be_bytes(hash.0),
        None => U256::ZERO,
    };
    node.outputs[0] = SSAOutput::Stack(value);
    Ok(())
}
