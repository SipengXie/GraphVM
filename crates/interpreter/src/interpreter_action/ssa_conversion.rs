use super::{CallInputs, CallOutcome, CallScheme, CreateInputs, CreateOutcome};
use crate::{return_ok, return_revert, Contract, InstructionResult, InterpreterResult};
use revm_primitives::CreateScheme;
use revm_ssa::{
    ContractEnv, SSACallInput, SSACallOutcome, SSACallScheme, SSACreateInput, SSACreateOutcome,
    SSACreateScheme, SSAInstructionResult, SSAInterpreterResult,
};

/// Convert interpreter's Contract to SSA's ContractEnv
pub fn convert_contract_env(env: &Contract) -> ContractEnv {
    ContractEnv {
        target_address: env.target_address,
        caller: env.caller,
        call_value: env.call_value,
        input: env.input.clone(),
        bytecode: env.bytecode.clone(),
        hash: env.hash,
        bytecode_address: env.bytecode_address,
    }
}

/// Convert interpreter's CallInputs to SSA's CallInput
pub fn convert_call_input(input: &CallInputs) -> SSACallInput {
    SSACallInput {
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
pub fn convert_create_input(input: &CreateInputs) -> SSACreateInput {
    SSACreateInput {
        caller: input.caller,
        value: input.value,
        init_code: input.init_code.clone(),
        scheme: convert_create_scheme(input.scheme),
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
fn convert_call_scheme(scheme: CallScheme) -> SSACallScheme {
    match scheme {
        CallScheme::Call => SSACallScheme::Call,
        CallScheme::CallCode => SSACallScheme::CallCode,
        CallScheme::DelegateCall => SSACallScheme::DelegateCall,
        CallScheme::StaticCall => SSACallScheme::StaticCall,
        CallScheme::ExtCall => SSACallScheme::ExtCall,
        CallScheme::ExtStaticCall => SSACallScheme::ExtStaticCall,
        CallScheme::ExtDelegateCall => SSACallScheme::ExtDelegateCall,
    }
}

/// Convert interpreter's CreateScheme to SSA's CreateScheme
fn convert_create_scheme(scheme: CreateScheme) -> SSACreateScheme {
    match scheme {
        CreateScheme::Create => SSACreateScheme::Create,
        CreateScheme::Create2 { salt } => SSACreateScheme::Create2 { salt },
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
