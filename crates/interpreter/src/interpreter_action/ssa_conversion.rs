use revm_primitives::{Address,CreateScheme};
use revm_ssa::{
    SSACallInput, SSACallOutcome,
     SSACallScheme, SSACreateInput, 
     SSACreateOutcome, SSACreateScheme, 
     SSAInstructionResult, SSAInterpreterResult};
use crate::{return_error, return_ok, return_revert, InterpreterResult, InstructionResult};
use super::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, CallScheme};


/// Convert interpreter's CallInputs to SSA's CallInput
pub fn convert_call_input(input: &CallInputs) -> SSACallInput {
    SSACallInput {
        input: input.input.clone(),
        target_address: input.target_address,
        bytecode_address: input.target_address,
        caller: input.caller,
        value: input.call_value(),
        scheme: convert_call_scheme(input.scheme),
        ret_range: input.return_memory_offset.clone(),
        code: None,
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
        target: Address::ZERO,
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
            return_error!() => SSAInstructionResult::Error,
            _ => SSAInstructionResult::Error,
        },
        output: result.output.clone(),
    }
}
