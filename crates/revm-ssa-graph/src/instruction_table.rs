use crate::{
    context::ExecutionContext, graph::SsaGraph, instructions::arithmetic::*,
    instructions::bitwise::*, instructions::contract::*, instructions::control::*,
    instructions::host::*, instructions::host_env::*, instructions::memory::*,
    instructions::system::*, Result,
};
use revm_primitives::{db::DatabaseRef, Spec};
use revm_ssa::{SSAInstructionResult, SSALogEntry};

pub type InstructionFn<DB> =
    fn(&mut ExecutionContext<DB>, &mut SSALogEntry, &SsaGraph) -> Result<()>;

pub struct InstructionTable<DB: DatabaseRef + Send + Sync> {
    pub instructions: [InstructionFn<DB>; 256],
}

impl<DB: DatabaseRef + Send + Sync> InstructionTable<DB> {
    /// Create instruction table for a specific spec
    pub fn create_instruction_table<SPEC: Spec>() -> Self {
        // 使用类型别名创建数组
        let mut instructions: [InstructionFn<DB>; 256] = [execute_invalid; 256];

        // Arithmetic Operations (0x00-0x0B)
        instructions[0x01] = execute_add; // ADD
        instructions[0x02] = execute_mul; // MUL
        instructions[0x03] = execute_sub; // SUB
        instructions[0x04] = execute_div; // DIV
        instructions[0x05] = execute_sdiv; // SDIV
        instructions[0x06] = execute_mod; // MOD
        instructions[0x07] = execute_smod; // SMOD
        instructions[0x08] = execute_addmod; // ADDMOD
        instructions[0x09] = execute_mulmod; // MULMOD
        instructions[0x0A] = execute_exp; // EXP
        instructions[0x0B] = execute_signextend; // SIGNEXTEND

        // Comparison & Bitwise Operations (0x10-0x1D)
        instructions[0x10] = execute_lt; // LT
        instructions[0x11] = execute_gt; // GT
        instructions[0x12] = execute_slt; // SLT
        instructions[0x13] = execute_sgt; // SGT
        instructions[0x14] = execute_eq; // EQ
        instructions[0x15] = execute_iszero; // ISZERO
        instructions[0x16] = execute_and; // AND
        instructions[0x17] = execute_or; // OR
        instructions[0x18] = execute_xor; // XOR
        instructions[0x19] = execute_not; // NOT
        instructions[0x1A] = execute_byte; // BYTE
        instructions[0x1B] = execute_shl; // SHL
        instructions[0x1C] = execute_shr; // SHR
        instructions[0x1D] = execute_sar; // SAR

        // SHA3 & Environmental Information (0x20-0x3F)
        instructions[0x20] = execute_keccak256; // KECCAK256
        instructions[0x30] = execute_address; // ADDRESS
        instructions[0x31] = execute_balance; // BALANCE
        instructions[0x32] = |ctx, node, graph| execute_host_env(ctx, node, graph, 0x32); // ORIGIN
        instructions[0x33] = execute_caller; // CALLER
        instructions[0x34] = execute_callvalue; // CALLVALUE
        instructions[0x35] = execute_calldataload; // CALLDATALOAD
        instructions[0x36] = execute_calldatasize; // CALLDATASIZE
        instructions[0x37] = execute_calldatacopy; // CALLDATACOPY
        instructions[0x38] = execute_codesize; // CODESIZE
        instructions[0x39] = execute_codecopy; // CODECOPY
        instructions[0x3A] = |ctx, node, graph| execute_host_env(ctx, node, graph, 0x3A); // GASPRICE
        instructions[0x3B] = execute_extcodesize; // EXTCODESIZE
        instructions[0x3C] = execute_extcodecopy; // EXTCODECOPY
        instructions[0x3D] = execute_returndatasize; // RETURNDATASIZE
        instructions[0x3E] = execute_returndatacopy; // RETURNDATACOPY
        instructions[0x3F] = execute_extcodehash; // EXTCODEHASH

        // Block Information (0x40-0x4A)
        instructions[0x40] = execute_blockhash; // BLOCKHASH
        instructions[0x41] = |ctx, node, graph| execute_host_env(ctx, node, graph, 0x41); // COINBASE
        instructions[0x42] = |ctx, node, graph| execute_host_env(ctx, node, graph, 0x42); // TIMESTAMP
        instructions[0x43] = |ctx, node, graph| execute_host_env(ctx, node, graph, 0x43); // NUMBER
        instructions[0x44] = |ctx, node, graph| execute_host_env(ctx, node, graph, 0x44); // DIFFICULTY
        instructions[0x45] = |ctx, node, graph| execute_host_env(ctx, node, graph, 0x45); // GASLIMIT
        instructions[0x46] = |ctx, node, graph| execute_host_env(ctx, node, graph, 0x46); // CHAINID
        instructions[0x47] = execute_selfbalance; // SELFBALANCE
        instructions[0x48] = |ctx, node, graph| execute_host_env(ctx, node, graph, 0x48); // BASEFEE
        instructions[0x49] = execute_blobhash; // BLOBHASH
        instructions[0x4A] = |ctx, node, graph| execute_host_env(ctx, node, graph, 0x4A); // BLOBBASEFEE

        // Stack, Memory, Storage and Flow Operations (0x50-0x5F)
        instructions[0x51] = execute_mload; // MLOAD
        instructions[0x52] = execute_mstore; // MSTORE
        instructions[0x53] = execute_mstore8; // MSTORE8
        instructions[0x54] = execute_sload; // SLOAD
        instructions[0x55] = execute_sstore::<DB, SPEC>; // SSTORE
        instructions[0x56] = execute_jump; // JUMP
        instructions[0x57] = execute_jumpi; // JUMPI
        instructions[0x59] = execute_msize; // MSIZE
        instructions[0x5A] = execute_gas; // GAS
        instructions[0x5C] = execute_tload; // TLOAD
        instructions[0x5D] = execute_tstore; // TSTORE
        instructions[0x5E] = execute_mcopy; // MCOPY

        // Logging Operations (0xA0-0xA4)
        instructions[0xA0] = execute_log; // LOG0
        instructions[0xA1] = execute_log; // LOG1
        instructions[0xA2] = execute_log; // LOG2
        instructions[0xA3] = execute_log; // LOG3
        instructions[0xA4] = execute_log; // LOG4

        // Internal Operations (0xD4-0xD9)
        instructions[0xD4] = execute_make_create_frame; // MAKE_CREATE_FRAME
        instructions[0xD5] = execute_create_return; // CREATE_RETURN
        instructions[0xD6] = execute_insert_create_outcome; // INSERT_CREATE_OUTCOME
        instructions[0xD7] = execute_make_call_frame; // MAKE_CALL_FRAME
        instructions[0xD8] = execute_call_return; // CALL_RETURN
        instructions[0xD9] = execute_insert_call_outcome; // INSERT_CALL_OUTCOME
        instructions[0xDA] = execute_deduct_caller; // DEDUCT_CALLER
        instructions[0xDB] = execute_refund_gas::<DB, SPEC>; // REFUND_GAS
        instructions[0xDC] = execute_reward_beneficiary; // REWARD_BENEFICIARY

        // System Operations (0xF0-0xFF)
        instructions[0xF0] = execute_create; // CREATE
        instructions[0xF1] = |ctx, node, graph| execute_call(ctx, node, graph, 0xF1); // CALL
        instructions[0xF2] = |ctx, node, graph| execute_callcode(ctx, node, graph, 0xF2); // CALLCODE
        instructions[0xF3] =
            |ctx, node, graph| execute_ret(ctx, node, graph, SSAInstructionResult::Ok); // RETURN
        instructions[0xF4] = |ctx, node, graph| execute_delegatecall(ctx, node, graph, 0xF4); // DELEGATECALL
        instructions[0xF5] = execute_create; // CREATE2
        instructions[0xFA] = |ctx, node, graph| execute_staticcall(ctx, node, graph, 0xFA); // STATICCALL
        instructions[0xFD] =
            |ctx, node, graph| execute_ret(ctx, node, graph, SSAInstructionResult::Revert); // REVERT
        instructions[0xFE] =
            |ctx, node, graph| execute_change_instruction_result(ctx, node, graph, 0xFE); // INVALID
        instructions[0xFF] = execute_selfdestruct; // SELFDESTRUCT

        Self { instructions }
    }
}

/// Default invalid instruction handler
fn execute_invalid<DB: DatabaseRef + Send + Sync>(
    _ctx: &mut ExecutionContext<DB>,
    _node: &mut SSALogEntry,
    _graph: &SsaGraph,
) -> Result<()> {
    Ok(())
}
