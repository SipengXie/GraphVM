use super::{CallInputs, CallOutcome, CallScheme, CreateInputs, CreateOutcome};
use crate::{return_ok, return_revert, Contract, InstructionResult, InterpreterResult};
use revm_primitives::CreateScheme;
use revm_ssa::{
    ContractEnv, FrameInput, SSACallOutcome, SSACreateOutcome, SSAInstructionResult,
    SSAInterpreterResult, TxScheme,
};

/// Convert interpreter's Contract to SSA's ContractEnv
pub fn convert_contract_env(env: &Contract, frame_input: FrameInput) -> ContractEnv {
    ContractEnv {
        bytecode: env.bytecode.clone(),
        hash: env.hash,
        frame_input,
    }
}

/// Convert interpreter's Contract to SSA's ContractEnv
pub fn convert_contract_env_for_system(env: &Contract) -> ContractEnv {
    ContractEnv {
        bytecode: env.bytecode.clone(),
        hash: env.hash,
        frame_input: FrameInput {
            input: env.input.clone(),
            bytecode_address: env.bytecode_address.unwrap_or_default(),
            target_address: env.target_address,
            caller: env.caller,
            transfer_value: env.call_value,
            ..Default::default()
        },
    }
}

/// Convert interpreter's CallInputs to SSA's CallInput
pub fn convert_call_input(input: &CallInputs) -> FrameInput {
    FrameInput {
        input: input.input.clone(),
        target_address: input.target_address,
        bytecode_address: input.bytecode_address,
        caller: input.caller,
        transfer_value: input.transfer_value().unwrap_or_default(),
        scheme: convert_call_scheme(input.scheme),
        ret_range: input.return_memory_offset.clone(),
        gas_limit: input.gas_limit,
    }
}

/// Convert interpreter's CallOutcome to SSA's CallOutcome
pub fn convert_call_outcome(outcome: &CallOutcome) -> SSACallOutcome {
    SSACallOutcome {
        result: convert_interpreter_result(&outcome.result),
        ret_range: outcome.memory_offset.clone(),
    }
}

/// Convert interpreter's CreateInputs to SSA's CreateInput
pub fn convert_create_input(input: &CreateInputs) -> FrameInput {
    FrameInput {
        caller: input.caller,
        transfer_value: input.value,
        input: input.init_code.clone(),
        scheme: convert_create_scheme(input.scheme),
        ..Default::default()
    }
}

/// Convert interpreter's CreateOutcome to SSA's CreateOutcome
pub fn convert_create_outcome(outcome: &CreateOutcome) -> SSACreateOutcome {
    SSACreateOutcome {
        result: convert_interpreter_result(&outcome.result),
        address: outcome.address,
    }
}

/// Convert interpreter's CallScheme to SSA's CallScheme
fn convert_call_scheme(scheme: CallScheme) -> TxScheme {
    match scheme {
        CallScheme::Call => TxScheme::Call,
        CallScheme::CallCode => TxScheme::CallCode,
        CallScheme::DelegateCall => TxScheme::DelegateCall,
        CallScheme::StaticCall => TxScheme::StaticCall,
        _ => unimplemented!(),
    }
}

/// Convert interpreter's CreateScheme to SSA's CreateScheme
fn convert_create_scheme(scheme: CreateScheme) -> TxScheme {
    match scheme {
        CreateScheme::Create => TxScheme::Create,
        CreateScheme::Create2 { salt } => TxScheme::Create2 { salt },
    }
}

pub fn convert_interpreter_result(result: &InterpreterResult) -> SSAInterpreterResult {
    SSAInterpreterResult {
        result: match result.result {
            return_ok!() => SSAInstructionResult::Ok,
            return_revert!() => SSAInstructionResult::Revert,
            InstructionResult::FatalExternalError => SSAInstructionResult::Error,
            _ => SSAInstructionResult::Revert,
        },
        output: result.output.clone(),
    }
}
