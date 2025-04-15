use crate::{context::{get_account_context, CallOutcome, CreateOutcome, ExternalContext, FrameContext, FrameInput}, typed_graph::{HasInputType, HasOutputType, TypedNode}, u256_to_address};
use revm_interpreter::{analysis::to_analysed, as_u64_saturated, as_usize_saturated, return_ok, InstructionResult, SharedMemory};
use revm_primitives::{
    keccak256, AccountInfo, AccountStatus, Address, Bytecode, Bytes, B256, U256, U256_ONE
};
use revm_ssa::TxScheme;
use std::{cell::RefCell, cmp::min, rc::Rc};

use super::memory::calc_memory_size;



// --- Simplified Deduct Caller Node ---
// Primarily handles nonce increment for non-CREATE calls. Balance deduction is complex without gas.

/// Node to handle caller state changes (nonce increment). Simplified without gas.
pub struct DeductCallerNode {
    /// Inputs:
    /// 0: *const Address - Caller address.
    /// 1: bool - True if the operation is CREATE/CREATE2 (prevents nonce increment).
    /// 2: *const U256 - Cost associated (e.g., gas cost, ignored for balance deduction now).
    /// 3: Rc<RefCell<ExternalContext>> - Reference to external context to get initial state.
    inputs: (*const Address, bool, *const U256, Rc<RefCell<ExternalContext>>),
    /// Output:
    /// 0: AccountInfo - Updated caller info (primarily nonce potentially incremented).
    outputs: (AccountInfo,),
}

impl HasInputType<(*const Address, bool, *const U256, Rc<RefCell<ExternalContext>>)> for DeductCallerNode {}
impl HasOutputType<(AccountInfo,)> for DeductCallerNode {}

impl DeductCallerNode {
    pub fn new(
        caller_ptr: *const Address,
        is_create: bool,
        cost_ptr: *const U256, // Added cost input, though unused for balance now
        context_ref: Rc<RefCell<ExternalContext>>
    ) -> Self {
        Self { inputs: (caller_ptr, is_create, cost_ptr, context_ref), outputs: (AccountInfo::default(),) }
    }
}

impl TypedNode for DeductCallerNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let caller_address = *self.inputs.0;
            let is_create = self.inputs.1;
            let cost = *self.inputs.2; // Read cost, but ignore it for balance modification for now
            let context_ref = &self.inputs.3;

            // Get original caller info from the external context using the address
            let (original_info, _original_status) = { // Read-only borrow needed
                 let context = context_ref.borrow(); // Borrow ExternalContext immutably
                 get_account_context(&context, caller_address) // Fetch info
            };


            let mut updated_info = original_info.clone();

            // Increment nonce only for non-create calls initiated by this frame
            if !is_create {
                updated_info.nonce = original_info.nonce.saturating_add(1);
            }

            // Balance deduction based on _cost is skipped as gas is ignored.
            // If balance deduction were needed:
            updated_info.balance = original_info.balance.saturating_sub(cost);

            // Store the potentially updated info as the output of this node
            self.outputs.0 = updated_info;

        }
        Ok(())
    }

    // Add getter for AccountInfo if needed
     fn get_account_info_output(&self, index: usize) -> Option<*const AccountInfo> {
         match index { 0 => Some(&self.outputs.0 as *const AccountInfo), _ => None }
     }
}

// --- Simplified Refund Gas Node ---
// Placeholder or very basic logic as dynamic gas is ignored.

/// Placeholder Node for Gas Refund logic (simplified).
pub struct RefundGasNode {
    // Inputs might include caller, initial gas, final gas, etc.
    // But simplified version might just be a no-op for now.
    _inputs: (), // Placeholder
    _outputs: (), // Placeholder
}
// impl HasInputType... HasOutputType...
impl HasInputType<()> for RefundGasNode {}
impl HasOutputType<()> for RefundGasNode {}
impl RefundGasNode { pub fn new() -> Self { Self { _inputs: (), _outputs: () } } }
impl TypedNode for RefundGasNode { fn execute(&mut self) -> anyhow::Result<()> { /* No-op */ Ok(()) } }


// --- Base Call Node Logic (Helper) ---
// Contains common logic for CALL, CALLCODE, DELEGATECALL, STATICCALL

fn execute_base_call(
    opcode: u8,
    inputs: &(*const U256, *const U256, *const U256, *const U256, *const U256, *const U256, *const U256, Rc<RefCell<SharedMemory>>, *const FrameContext),
    outputs: &mut (FrameInput,)
) -> anyhow::Result<()> {
     unsafe {
        let gas_limit_ptr = inputs.0;
        let to_ptr = inputs.1;
        let value_ptr = inputs.2; // Used in CALL/CALLCODE, ignored in DELEGATE/STATIC
        let in_offset_ptr = inputs.3;
        let in_len_ptr = inputs.4;
        let out_offset_ptr = inputs.5;
        let out_len_ptr = inputs.6;
        let memory_ref = inputs.7.as_ref();
        let frame_ptr = inputs.8;

        let gas_limit = as_u64_saturated!(*gas_limit_ptr);
        let to_u256 = *to_ptr;
        let value = if opcode == 0xF1 || opcode == 0xF2 { *value_ptr } else { U256::ZERO }; // Only CALL/CALLCODE transfer value
        let in_offset = as_usize_saturated!(*in_offset_ptr);
        let in_len = as_usize_saturated!(*in_len_ptr);
        let out_offset = as_usize_saturated!(*out_offset_ptr);
        let out_len = as_usize_saturated!(*out_len_ptr);
        let current_frame = &*frame_ptr;

        // --- Read input data from memory ---
        let input_data = {
            let mut memory = memory_ref.borrow_mut();
            let required_in_size = calc_memory_size(in_offset, in_len);
             if required_in_size > memory.len() { memory.resize(required_in_size); }
             Bytes::copy_from_slice(memory.slice(in_offset, in_len))
        };

         // --- Ensure memory is large enough for output buffer ---
         { // Separate scope for mutable borrow
             let mut memory = memory_ref.borrow_mut();
             let required_out_size = calc_memory_size(out_offset, out_len);
             if required_out_size > memory.len() { memory.resize(required_out_size); }
         }

        let scheme = match opcode {
            0xF1 => TxScheme::Call,
            0xF2 => TxScheme::CallCode,
            0xF4 => TxScheme::DelegateCall,
            0xFA => TxScheme::StaticCall,
            _ => return Err(anyhow::anyhow!("Invalid call opcode: {:x}", opcode)),
        };

        let (target_address, caller, bytecode_address) = match scheme {
            TxScheme::Call | TxScheme::StaticCall => {
                let target = u256_to_address!(to_u256);
                (target, current_frame.frame_input.target_address, target)
            }
            TxScheme::CallCode => {
                (current_frame.frame_input.target_address, current_frame.frame_input.target_address, u256_to_address!(to_u256))
            }
            TxScheme::DelegateCall => {
                 (current_frame.frame_input.target_address, current_frame.frame_input.caller, u256_to_address!(to_u256)) // Caller and value preserved
            }
            _ => unreachable!(), // Should be covered by opcode match
        };

        outputs.0 = FrameInput {
            input: input_data,
            target_address,
            bytecode_address,
            caller,
            transfer_value: if scheme == TxScheme::DelegateCall { U256::ZERO } else { value }, // DELEGATECALL does not transfer value
            scheme,
            ret_range: out_offset..out_offset + out_len,
            gas_limit,
            is_static: scheme == TxScheme::StaticCall, // TODO:Inherit static
        };
    }
    Ok(())
}


// --- CALL Node (0xf1) ---
pub struct CallNode {
    inputs: (*const U256, *const U256, *const U256, *const U256, *const U256, *const U256, *const U256, Rc<RefCell<SharedMemory>>, *const FrameContext),
    outputs: (FrameInput,),
}
type CallInputs = (*const U256, *const U256, *const U256, *const U256, *const U256, *const U256, *const U256, Rc<RefCell<SharedMemory>>, *const FrameContext);
impl HasInputType<CallInputs> for CallNode {}
impl HasOutputType<(FrameInput,)> for CallNode {}
impl CallNode { pub fn new(i: CallInputs) -> Self { Self { inputs: i, outputs: (FrameInput::default(),) } } }
impl TypedNode for CallNode {
    fn execute(&mut self) -> anyhow::Result<()> { execute_base_call(0xF1, &self.inputs, &mut self.outputs) }
    fn get_frame_input_output(&self) -> Option<*const FrameInput> {
        Some(&self.outputs.0 as *const FrameInput)
    }
}


// --- CALLCODE Node (0xf2) ---
pub struct CallcodeNode {
    inputs: (*const U256, *const U256, *const U256, *const U256, *const U256, *const U256, *const U256, Rc<RefCell<SharedMemory>>, *const FrameContext),
    outputs: (FrameInput,),
}
impl HasInputType<CallInputs> for CallcodeNode {} // Reuses CallInputs type alias
impl HasOutputType<(FrameInput,)> for CallcodeNode {}
impl CallcodeNode { pub fn new(i: CallInputs) -> Self { Self { inputs: i, outputs: (FrameInput::default(),) } } }
impl TypedNode for CallcodeNode {
    fn execute(&mut self) -> anyhow::Result<()> { execute_base_call(0xF2, &self.inputs, &mut self.outputs) }
    fn get_frame_input_output(&self) -> Option<*const FrameInput> {
        Some(&self.outputs.0 as *const FrameInput)
    }
}


// --- DELEGATECALL Node (0xf4) ---
pub struct DelegatecallNode {
    // Delegatecall doesn't use 'value' input, so input tuple is slightly different
    inputs: (*const U256, *const U256, *const U256, *const U256, *const U256, *const U256, Rc<RefCell<SharedMemory>>, *const FrameContext),
    outputs: (FrameInput,),
}
type DelegatecallInputs = (*const U256, *const U256, *const U256, *const U256, *const U256, *const U256, Rc<RefCell<SharedMemory>>, *const FrameContext);
impl HasInputType<DelegatecallInputs> for DelegatecallNode {}
impl HasOutputType<(FrameInput,)> for DelegatecallNode {}
impl DelegatecallNode { pub fn new(i: DelegatecallInputs) -> Self { Self { inputs: i, outputs: (FrameInput::default(),) } } }
impl TypedNode for DelegatecallNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        // Adapt inputs for base call function (insert a dummy value pointer)
        let dummy_value = U256::ZERO;
        let base_inputs = (
            self.inputs.0, self.inputs.1, &dummy_value as *const U256, // gas, to, dummy_value
            self.inputs.2, self.inputs.3, self.inputs.4, self.inputs.5, // in_offset, in_len, out_offset, out_len
            self.inputs.6.clone(), self.inputs.7 // memory, frame
        );
        execute_base_call(0xF4, &base_inputs, &mut self.outputs)
    }
    fn get_frame_input_output(&self) -> Option<*const FrameInput> {
        Some(&self.outputs.0 as *const FrameInput)
    }
}


// --- STATICCALL Node (0xfa) ---
pub struct StaticcallNode {
    // Staticcall also doesn't use 'value' input
    inputs: (*const U256, *const U256, *const U256, *const U256, *const U256, *const U256, Rc<RefCell<SharedMemory>>, *const FrameContext),
    outputs: (FrameInput,),
}
impl HasInputType<DelegatecallInputs> for StaticcallNode {} // Reuses DelegatecallInputs type alias
impl HasOutputType<(FrameInput,)> for StaticcallNode {}
impl StaticcallNode { pub fn new(i: DelegatecallInputs) -> Self { Self { inputs: i, outputs: (FrameInput::default(),) } } }
impl TypedNode for StaticcallNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        // Adapt inputs for base call function
        let dummy_value = U256::ZERO;
        let base_inputs = (
            self.inputs.0, self.inputs.1, &dummy_value as *const U256,
            self.inputs.2, self.inputs.3, self.inputs.4, self.inputs.5,
            self.inputs.6.clone(), self.inputs.7
        );
        execute_base_call(0xFA, &base_inputs, &mut self.outputs)
    }
    fn get_frame_input_output(&self) -> Option<*const FrameInput> {
        Some(&self.outputs.0 as *const FrameInput)
    }
}


// --- Make Call Frame Node (Conceptual) ---
// Sets up context before executing a sub-graph based on FrameInput.
// In a real executor, this might not be a distinct node but part of the executor logic.
// If implemented as a node, it needs careful state management.

pub struct MakeCallFrameNode {
    /// Inputs:
    /// 0: *const FrameInput - The call parameters.
    /// 1: Option<*const AccountInfo> - Caller account info (if it is updated by other nodes)
    /// 2: Option<*const AccountInfo> - Target account info (if it is updated by other nodes)
    /// 3: Option<*const AccountInfo> - Bytecode address account info (if it is updated by other nodes)
    /// 4: Option<Rc<RefCell<ExternalContext>>> - To get account info and potentially update balances.
    inputs: (*const FrameInput, Option<*const AccountInfo>, Option<*const AccountInfo>, Option<*const AccountInfo>, Rc<RefCell<ExternalContext>>),
    /// Outputs:
    /// 0: Option<FrameContext> - The context for the next frame (if it's a contract call). None if precompile/empty.
    /// 1: Option<CallOutcome> - The direct result if it's a precompile or empty call.
    /// 2: Option<AccountInfo> - Updated caller info (if value transfer).
    /// 3: Option<AccountInfo> - Updated target info (if value transfer).
    outputs: (Option<FrameContext>, Option<CallOutcome>, Option<AccountInfo>, Option<AccountInfo>),
}
// Define Input/Output types and impl Has... traits
type MakeCallFrameInputs = (*const FrameInput, Option<*const AccountInfo>, Option<*const AccountInfo>, Option<*const AccountInfo>, Rc<RefCell<ExternalContext>>);
type MakeCallFrameOutputs = (Option<FrameContext>, Option<CallOutcome>, Option<AccountInfo>, Option<AccountInfo>);
impl HasInputType<MakeCallFrameInputs> for MakeCallFrameNode {}
impl HasOutputType<MakeCallFrameOutputs> for MakeCallFrameNode {}

impl MakeCallFrameNode {
    pub fn new(
        frame_input_ptr: *const FrameInput,
        caller_info_ptr: Option<*const AccountInfo>,
        target_info_ptr: Option<*const AccountInfo>,
        bytecode_info_ptr: Option<*const AccountInfo>,
        context_ref: Rc<RefCell<ExternalContext>>
    ) -> Self {
        Self {
            inputs: (frame_input_ptr, caller_info_ptr, target_info_ptr, bytecode_info_ptr, context_ref),
            outputs: (None, None, None, None), // Initialize outputs
        }
    }
}
impl TypedNode for MakeCallFrameNode {
     fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            // 1. Get FrameInput and context.
            let frame_input = &*self.inputs.0;
            let caller = frame_input.caller;
            let target_address = frame_input.target_address;
            let bytecode_address = frame_input.bytecode_address;
            let value = frame_input.transfer_value;
            let input = &frame_input.input;
            let gas_limit = frame_input.gas_limit;
            let context_borrow = self.inputs.4.borrow(); // Need mutable borrow for potential updates


            // --- State Updates for Value Transfer (Conceptual) ---
            // This node *outputs* the potentially updated AccountInfo.
            // The actual state update should be handled by the executor
            // or a dedicated state-committing mechanism based on these outputs.
            if !value.is_zero() {

                // Clone the AccountInfo from the inputs.
                let mut caller_info = self.inputs.1.map_or_else(
                    || get_account_context(&context_borrow, caller).0,
                    |ptr|  (*ptr).clone() 
                );
                let mut target_info = self.inputs.2.map_or_else(
                    || get_account_context(&context_borrow, target_address).0,
                    |ptr|  (*ptr).clone() 
                );
                caller_info.balance = caller_info.balance.saturating_sub(value);
                target_info.balance = target_info.balance.saturating_add(value);
                
                // Store the *potentially* updated infos.
                self.outputs.2 = Some(caller_info);
                self.outputs.3 = Some(target_info);
            } else {
                 // If no value transfer, outputs remain None or could be clones of original.
                 // Let's keep them None for now, implying no state change *caused* by value transfer.
                 self.outputs.2 = None;
                 self.outputs.3 = None;
            }

            let bytecode = self.inputs.3.map_or_else(
                || get_account_context(&context_borrow, bytecode_address).0.code.clone().unwrap_or_default(),
                |ptr|  (*ptr).code.clone().unwrap_or_default() 
            );

            // 2. Check Precompile
            if context_borrow.is_precompile(&bytecode_address) {
                // Call precompile using the context.
                // Assuming call_precompile returns (InstructionResult, Bytes, u64) -> (result, output, gas_used)
                let (result, output) = context_borrow.call_precompile(&bytecode_address, input, gas_limit);
                // Produce CallOutcome for precompile execution.
                // TODO: gas_used should be returned from call_precompile.
                self.outputs.1 = Some(CallOutcome { result, return_data: output, ret_range: frame_input.ret_range.clone() });
                self.outputs.0 = None; // No FrameContext for precompile
            }
            // 3. Check Empty Bytecode (Simple Transfer or Non-Contract Account)
            else if bytecode.is_empty() {
                 // Produce CallOutcome indicating success (simple transfer).
                 // Gas cost for simple transfers is handled elsewhere.
                 // TODO: gas_used should be returned from call_precompile.
                 self.outputs.1 = Some(CallOutcome {
                    result: InstructionResult::Stop, // Assuming InstructionResult::Ok exists
                    return_data: Bytes::default(),
                    ret_range: 0..0,
                 });
                 self.outputs.0 = None; // No FrameContext for empty code
            }
            // 4. Contract Call
            else {
                // Create FrameContext for the sub-call.
                // Assuming FrameContext is similar to revm_ssa::ContractEnv
                let frame_context = FrameContext {
                    frame_input: frame_input.clone(), // Clone or copy frame_input
                    bytecode: bytecode.clone(),
                    hash: Some(bytecode.hash_slow()), // Use the hash from bytecode_info
                    
                };
                self.outputs.0 = Some(frame_context);
                self.outputs.1 = None; // No direct CallOutcome, execution happens in sub-graph.
            }
        }
        Ok(())
     }

     fn get_frame_context_output(&self) -> Option<*const FrameContext> {
         self.outputs.0.as_ref().map(|fc| fc as *const FrameContext)
     }

     fn get_call_outcome_output(&self) -> Option<*const CallOutcome> {
         self.outputs.1.as_ref().map(|co| co as *const CallOutcome)
     }

     fn get_account_info_output(&self, index: usize) -> Option<*const AccountInfo> {
         match index {
            2 => self.outputs.2.as_ref().map(|ai| ai as *const AccountInfo),
            3 => self.outputs.3.as_ref().map(|ai| ai as *const AccountInfo),
            _ => None,
         }
     }
}


// --- Call Return Node (Conceptual) ---
// Processes the result of a sub-graph execution.

pub struct CallReturnNode {
    /// Inputs:
    /// 0: InstructionResult - Result status from sub-execution.
    /// 1: *const Bytes - Return data buffer from sub-execution.
    /// 3: *const FrameContext - Context of the *completed* sub-frame.
    inputs: (InstructionResult, *const Bytes, *const FrameContext),
    /// Outputs:
    /// 0: CallOutcome - Bundled result.
    outputs: (CallOutcome,),
}
// Define Input/Output types and impl Has... traits
type CallReturnInputs = (InstructionResult, *const Bytes, *const FrameContext);
type CallReturnOutputs = (CallOutcome,);
impl HasInputType<CallReturnInputs> for CallReturnNode {}
impl HasOutputType<CallReturnOutputs> for CallReturnNode {}

impl CallReturnNode {
    pub fn new(
        result: InstructionResult,
        return_data_ptr: *const Bytes,
        frame_context_ptr: *const FrameContext
    ) -> Self {
        Self {
            inputs: (result, return_data_ptr, frame_context_ptr),
            // Initialize outputs with placeholder values that will be overwritten
            outputs: (CallOutcome { result: InstructionResult::Continue, return_data: Bytes::new(), ret_range: 0..0 },),
        }
    }
}
impl TypedNode for CallReturnNode {
     fn execute(&mut self) -> anyhow::Result<()> {
         unsafe {
             let result = self.inputs.0;
             let return_data = (*self.inputs.1).clone();
             let frame_context = &*self.inputs.2;
             // FrameContext might be needed to know the original ret_range if not stored elsewhere
             self.outputs.0 = CallOutcome { result, return_data, ret_range: frame_context.frame_input.ret_range.clone() };
         }
         Ok(())
     }

     fn get_call_outcome_output(&self) -> Option<*const CallOutcome> {
        Some(&self.outputs.0 as *const CallOutcome)
     }
}


// --- Insert Call Outcome Node ---
// Takes CallOutcome and applies it: writes return data to memory, sets status.

pub struct InsertCallOutcomeNode {
    /// Inputs:
    /// 0: *const CallOutcome - The result from the call.
    /// 1: Rc<RefCell<SharedMemory>> - Memory to write return data into.
    /// 2: *const FrameContext - The *original* frame context that initiated the call (needed for ret_range).
    inputs: (*const CallOutcome, Rc<RefCell<SharedMemory>>, *const FrameContext),
    /// Outputs:
    /// 0: Bytes - The return data buffer (for RETURNDATASIZE/COPY).
    /// 1: U256 - Success status (1 for Ok, 0 for Revert/Error).
    outputs: (Bytes, U256),
}
// Define Input/Output types and impl Has... traits
type InsertCallOutcomeInputs = (*const CallOutcome, Rc<RefCell<SharedMemory>>, *const FrameContext);
type InsertCallOutcomeOutputs = (U256, Bytes);
impl HasInputType<InsertCallOutcomeInputs> for InsertCallOutcomeNode {}
impl HasOutputType<InsertCallOutcomeOutputs> for InsertCallOutcomeNode {}

impl InsertCallOutcomeNode {
    pub fn new(
        outcome_ptr: *const CallOutcome,
        memory_ref: Rc<RefCell<SharedMemory>>,
        original_frame_ptr: *const FrameContext
    ) -> Self {
        Self {
            inputs: (outcome_ptr, memory_ref, original_frame_ptr),
            outputs: (Bytes::default(), U256::ZERO), // Initialize outputs
        }
    }
}
impl TypedNode for InsertCallOutcomeNode {
    fn execute(&mut self) -> anyhow::Result<()> {
         unsafe {
             let outcome = &*self.inputs.0;
             let mut memory = self.inputs.1.borrow_mut();
             let original_frame = &*self.inputs.2; // Frame that made the call

             let success = matches!(outcome.result, return_ok!());
             let return_data = &outcome.return_data;
             let ret_range = original_frame.frame_input.ret_range.clone(); // Get range from original frame

             // Write return data to memory respecting the range
             let write_len = min(ret_range.len(), return_data.len());
             if write_len > 0 {
                 let mem_offset = ret_range.start;
                  // Ensure memory exists (resize happens in memory methods)
                 memory.set(mem_offset, &return_data[..write_len]);
             }

             self.outputs.0 = return_data.clone(); // Store for RETURNDATA ops
             self.outputs.1 = if success { U256_ONE } else { U256::ZERO };
         }
         Ok(())
    }

    fn get_bytes_output(&self) -> Option<*const Bytes> {
        Some(&self.outputs.0 as *const Bytes)
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.1 as *const U256
    }
    
}


// --- CREATE / CREATE2 Nodes ---
// These nodes prepare a FrameInput for a creation context.

/// Node for CREATE operation.
pub struct CreateNode {
    /// Inputs:
    /// 0: *const U256 - Value to transfer.
    /// 1: *const U256 - Memory offset for init code.
    /// 2: *const U256 - Length of init code.
    /// 3: Rc<RefCell<SharedMemory>> - Shared memory reference.
    /// 4: *const FrameContext - Current frame context (for caller, nonce).
    inputs: (*const U256, *const U256, *const U256, Rc<RefCell<SharedMemory>>, *const FrameContext),
    /// Output:
    /// 0: FrameInput - Input for the creation frame.
    outputs: (FrameInput,),
}
impl HasInputType<(*const U256, *const U256, *const U256, Rc<RefCell<SharedMemory>>, *const FrameContext)> for CreateNode {}
impl HasOutputType<(FrameInput,)> for CreateNode {}

impl CreateNode {
     pub fn new(
         value_ptr: *const U256,
         code_offset_ptr: *const U256,
         len_ptr: *const U256,
         memory_ref: Rc<RefCell<SharedMemory>>,
         frame_ptr: *const FrameContext
    ) -> Self {
         Self { inputs: (value_ptr, code_offset_ptr, len_ptr, memory_ref, frame_ptr), outputs: (FrameInput::default(),) }
     }
}
impl TypedNode for CreateNode {
     fn execute(&mut self) -> anyhow::Result<()> {
         unsafe {
             let value = *self.inputs.0;
             let code_offset = as_usize_saturated!(*self.inputs.1);
             let len = as_usize_saturated!(*self.inputs.2);
             let mut memory = self.inputs.3.borrow_mut(); // Mutable borrow to resize
             let current_frame = &*self.inputs.4;

             // Read init code from memory
             let required_size = calc_memory_size(code_offset, len);
             if required_size > memory.len() { memory.resize(required_size); }
             let init_code = Bytes::copy_from_slice(memory.slice(code_offset, len));

             self.outputs.0 = FrameInput {
                 input: init_code,
                 caller: current_frame.frame_input.target_address,
                 transfer_value: value,
                 scheme: TxScheme::Create, // CREATE
                 ..Default::default()
             };
         }
         Ok(())
     }
    fn get_frame_input_output(&self) -> Option<*const FrameInput> {
        Some(&self.outputs.0 as *const FrameInput)
    }
}


/// Node for CREATE2 operation.
pub struct Create2Node {
    /// Inputs:
    /// 0: *const U256 - Value to transfer.
    /// 1: *const U256 - Memory offset for init code.
    /// 2: *const U256 - Length of init code.
    /// 3: *const U256 - Salt value.
    /// 4: Rc<RefCell<SharedMemory>> - Shared memory reference.
    /// 5: *const FrameContext - Current frame context (for caller).
    inputs: (*const U256, *const U256, *const U256, *const U256, Rc<RefCell<SharedMemory>>, *const FrameContext),
     /// Output:
    /// 0: FrameInput - Input for the creation frame.
    outputs: (FrameInput,),
}
impl HasInputType<(*const U256, *const U256, *const U256, *const U256, Rc<RefCell<SharedMemory>>, *const FrameContext)> for Create2Node {}
impl HasOutputType<(FrameInput,)> for Create2Node {}

impl Create2Node {
     pub fn new(
        value_ptr: *const U256,
        code_offset_ptr: *const U256,
        len_ptr: *const U256,
        salt_ptr: *const U256,
        memory_ref: Rc<RefCell<SharedMemory>>,
        frame_ptr: *const FrameContext
    ) -> Self {
        Self { inputs: (value_ptr, code_offset_ptr, len_ptr, salt_ptr, memory_ref, frame_ptr), outputs: (FrameInput::default(),) }
    }
}
impl TypedNode for Create2Node {
     fn execute(&mut self) -> anyhow::Result<()> {
         unsafe {
            let value = *self.inputs.0;
            let code_offset = as_usize_saturated!(*self.inputs.1);
            let len = as_usize_saturated!(*self.inputs.2);
            let salt = *self.inputs.3;
            let mut memory = self.inputs.4.borrow_mut();
            let current_frame = &*self.inputs.5;

             // Read init code from memory
            let required_size = calc_memory_size(code_offset, len);
            if required_size > memory.len() { memory.resize(required_size); }
            let init_code = Bytes::copy_from_slice(memory.slice(code_offset, len));

            self.outputs.0 = FrameInput {
                input: init_code,
                caller: current_frame.frame_input.target_address,
                transfer_value: value,
                scheme: TxScheme::Create2 { salt }, // CREATE2 with salt
                ..Default::default()
            };
         }
         Ok(())
     }
    fn get_frame_input_output(&self) -> Option<*const FrameInput> {
        Some(&self.outputs.0 as *const FrameInput)
    }
}

// --- Make Create Frame Node (Conceptual) ---
// Similar to MakeCallFrame, but calculates create address and sets initial nonce.

pub struct MakeCreateFrameNode {
     /// Inputs:
    /// 0: *const FrameInput - The create parameters.
    /// 1: Option<*const AccountInfo> - Caller's account info (needed for nonce, updated by other nodes).
    /// 2: Option<Rc<RefCell<ExternalContext>>> - To check target existence and update state, get caller's info if not touched by other nodes.
    inputs: (*const FrameInput, Option<*const AccountInfo>, Option<Rc<RefCell<ExternalContext>>>),
    /// Outputs:
    /// 0: FrameContext - Context for the create execution (if valid).
    /// 1: AccountInfo - Updated caller info (balance transfer, nonce).
    /// 2: AccountInfo - Contract account info.
    /// 3: AccountStatus - Initial status for the *created* address.
    outputs: (FrameContext, AccountInfo, AccountInfo, AccountStatus),
}
// Define Input/Output types and impl Has... traits

impl MakeCreateFrameNode {
    pub fn new(
        frame_input_ptr: *const FrameInput,
        caller_info_ptr: Option<*const AccountInfo>,
        context_ref_opt: Option<Rc<RefCell<ExternalContext>>>
    ) -> Self {
        assert_ne!(caller_info_ptr.is_some(), context_ref_opt.is_some(), "caller_info_ptr and context_ref_opt must not both be Some");
        Self {
            inputs: (frame_input_ptr, caller_info_ptr, context_ref_opt),
            outputs: (FrameContext::default(), AccountInfo::default(), AccountInfo::default(), AccountStatus::default()), // Initialize outputs
        }
    }
}
impl TypedNode for MakeCreateFrameNode {
      fn execute(&mut self) -> anyhow::Result<()> {
          unsafe {
                // 1. Get FrameInput, CallerInfo.
                let frame_input = &*self.inputs.0;
                
                let caller = frame_input.caller;
                let value = frame_input.transfer_value;

                let mut caller_info = self.inputs.1.map_or_else(
                    || 
                    {
                        let context_borrow = &*self.inputs.2.as_ref().unwrap().borrow();
                        get_account_context(&context_borrow, caller).0
                    },
                    |ptr|  (*ptr).clone() 
                );

                // 3. Calculate create address.
                let (target_address, init_code_hash) = match frame_input.scheme {
                    TxScheme::Create => (caller.create(caller_info.nonce), B256::ZERO),
                    TxScheme::Create2 { salt } => {
                        let hash = keccak256(&frame_input.input);
                        (caller.create2(salt.to_be_bytes(), hash), hash)
                    }
                    _ => return Err(anyhow::anyhow!("Invalid scheme for create frame")),
                };

                // Perform value transfer simulation and nonce increment for caller.
                caller_info.balance = caller_info.balance.saturating_sub(value);
                caller_info.nonce = caller_info.nonce.saturating_add(1); // Nonce increments upon successful submission of CREATE/CREATE2

                // 7. Set initial state for created contract.
                let initial_created_info = AccountInfo {
                    balance: value, // Start with transferred value
                    nonce: 1,       // Initial nonce for new contract is 1
                    code_hash: B256::ZERO, // Code hash is zero until deployment finishes
                    code: None,     // Code is initially empty
                };
                let initial_created_status = AccountStatus::Created; // Mark as loaded, will be updated by CreateReturnNode

                // 6. Create FrameContext for create execution.
                let bytecode = Bytecode::new_legacy(frame_input.input.clone());
                let mut frame_context = FrameContext {
                    frame_input: frame_input.clone(), // Pass relevant parts
                    bytecode,
                    hash: Some(init_code_hash), // Use calculated hash (Zero for CREATE)
                };
                frame_context.frame_input.target_address = target_address;

                // 8. Output FrameContext, updated caller, initial created state.
                self.outputs.0 = frame_context;
                self.outputs.1 = caller_info;
                self.outputs.2 = initial_created_info;
                self.outputs.3 = initial_created_status;
            }
            Ok(())
      }
      
      fn get_frame_context_output(&self) -> Option<*const FrameContext> {
          Some(&self.outputs.0 as *const FrameContext)
      }

      fn get_account_info_output(&self, _index: usize) -> Option<*const AccountInfo> {
          match _index {
            1 => Some(&self.outputs.1 as *const AccountInfo),
            2 => Some(&self.outputs.2 as *const AccountInfo),
            _ => None,
          }
      }

      fn get_account_status_output(&self) -> *const AccountStatus {
          &self.outputs.3 as *const AccountStatus
      }
}


// --- Create Return Node (Conceptual) ---
// Processes the result of a CREATE sub-graph execution. Deploys code.

pub struct CreateReturnNode {
     /// Inputs:
    /// 0: InstructionResult - Result status from sub-execution. If is not return_ok!, args 1~5 will be None.
    /// 1: Option<*const Bytes> - Deployment bytecode (output of sub-execution).
    /// 2: Option<*const FrameContext> - Context of the *completed* create sub-frame (contains calculated address).
    /// 3: Option<Rc<RefCell<ExternalContext>>> - To deploy the code (update account info).
    /// 4: Option<*const AccountInfo> - Target account info.
    /// 5: Option<bool> Whether to analyze the code.
    inputs: (InstructionResult, Option<*const Bytes>, Option<*const FrameContext>, Option<Rc<RefCell<ExternalContext>>>, Option<*const AccountInfo>, Option<bool>),
    /// Outputs:
    /// 0: CreateOutcome - Bundled result including the created address.
    /// 1: AccountInfo - Updated AccountInfo with deployed code (if successful).
    outputs: (CreateOutcome, AccountInfo),
}
// Define Input/Output types and impl Has... traits

impl CreateReturnNode {
    pub fn new(
        result: InstructionResult,
        output_ptr: Option<*const Bytes>,
        frame_context_ptr: Option<*const FrameContext>,
        ext_context_opt: Option<Rc<RefCell<ExternalContext>>>,
        target_info_ptr: Option<*const AccountInfo>,
        analyze_opt: Option<bool>,
    ) -> Self {
        if matches!(result, return_ok!()) {
            assert_ne!(target_info_ptr.is_some(), ext_context_opt.is_some(), "target_info_ptr and ext_context_opt must not both be Some");
        }
        Self {
            inputs: (result, output_ptr, frame_context_ptr, ext_context_opt, target_info_ptr, analyze_opt),
            outputs: (CreateOutcome::default(), AccountInfo::default()),
        }
    }
}
impl TypedNode for CreateReturnNode {
      fn execute(&mut self) -> anyhow::Result<()> {
          unsafe {
                // 1. Get result, deployment bytecode, gas used, frame context, initial state.
                let result = self.inputs.0;
                if !matches!(result, return_ok!()) {
                    return Err(anyhow::anyhow!("CreateReturnNode received non-return_ok! result"));
                }
                
                let deployment_code_bytes = &*self.inputs.1.unwrap();
                let frame_context = &*self.inputs.2.unwrap();
                let contract_address = frame_context.frame_input.target_address;

                let mut target_info = self.inputs.4.map_or_else(
                    || 
                    {
                        let context_borrow = &*self.inputs.3.as_ref().unwrap().borrow();
                        get_account_context(&context_borrow, contract_address).0
                    },
                    |ptr|  (*ptr).clone() 
                );

                let create_outcome = CreateOutcome {
                    result,
                    return_data: deployment_code_bytes.clone(),
                    created_address: Some(contract_address),
                };
                
                let analysis_kind = self.inputs.5.unwrap_or(false);
                let bytecode = if analysis_kind {
                    to_analysed(Bytecode::new_legacy(deployment_code_bytes.clone()))
                } else {
                    Bytecode::new_legacy(deployment_code_bytes.clone())
                };
                let codehash = bytecode.hash_slow();

                target_info.code_hash = codehash;
                target_info.code = Some(bytecode);

                self.outputs.0 = create_outcome;
                self.outputs.1 = target_info;

                Ok(())
        }
      }

      fn get_create_outcome_output(&self) -> Option<*const CreateOutcome> {
          Some(&self.outputs.0 as *const CreateOutcome)
      }

      fn get_account_info_output(&self, _index: usize) -> Option<*const AccountInfo> {
          Some(&self.outputs.1 as *const AccountInfo)
      }
}

// --- Insert Create Outcome Node ---
// Takes CreateOutcome and outputs address/status.

pub struct InsertCreateOutcomeNode {
    /// Input:
    /// 0: *const CreateOutcome - The result from the creation.
    inputs: (*const CreateOutcome,),
    /// Outputs:
    /// 0: Bytes - The return data buffer (only relevant on revert).
    /// 1: U256 - Created address as U256 (or 0 if failed/reverted).
    outputs: (Bytes, U256),
}
impl HasInputType<(*const CreateOutcome,)> for InsertCreateOutcomeNode {}
impl HasOutputType<(Bytes, U256)> for InsertCreateOutcomeNode {}

impl InsertCreateOutcomeNode {
     pub fn new(outcome_ptr: *const CreateOutcome) -> Self {
         Self { inputs: (outcome_ptr,), outputs: (Bytes::default(), U256::ZERO) }
     }
}
impl TypedNode for InsertCreateOutcomeNode {
      fn execute(&mut self) -> anyhow::Result<()> {
          unsafe {
              let outcome = &*self.inputs.0;
              let address_u256 = match outcome.result {
                  return_ok!() => {
                       // Success, output the address
                       outcome.created_address.map_or(U256::ZERO, |addr| U256::from_be_bytes(addr.into_word().0))
                  }
                  _ => U256::ZERO, // Failed or reverted, output 0
              };
              let return_data = if outcome.result.is_revert() {
                  outcome.return_data.clone()
              } else {
                  Bytes::default()
              };

              self.outputs.0 = return_data; // Store for RETURNDATA ops
              self.outputs.1 = address_u256;
          }
          Ok(())
      }
    
    fn get_bytes_output(&self) -> Option<*const Bytes> {
        Some(&self.outputs.0 as *const Bytes)
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.1 as *const U256
    }
}
