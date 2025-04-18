use crate::{
    context::{get_account_context, get_storage_slot_context, ExternalContext, FrameContext},
    typed_graph::TypedNode,
    u256_to_address,
};
use revm_interpreter::{as_u64_saturated, as_usize_saturated, InstructionResult, SharedMemory};
use revm_primitives::{AccountInfo, AccountStatus, Bytes, KECCAK_EMPTY, U256};
use std::{cell::RefCell, rc::Rc};
use super::types::*;

// --- SLOAD Node ---

/// Node for SLOAD operation. Reads a storage slot.
pub struct SloadNode {
    inputs: StorageLoadInputs,
    outputs: U256Output,
}


impl SloadNode {
    pub fn new(
        frame_context: *const FrameContext,
        index_ptr: *const U256,
        value_ptr: Option<*const U256>,
        context_ref: Rc<RefCell<ExternalContext>>,
    ) -> Self {
        Self {
            inputs: (frame_context, index_ptr, value_ptr, context_ref),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SloadNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            if let Some(value_ptr) = self.inputs.2 {
                // Value comes from a previous node in the graph
                self.outputs.0 = *value_ptr;
            } else {
                // Value comes from external context
                let address = (*self.inputs.0).frame_input.target_address;
                let index = *self.inputs.1;
                let context_ref = &self.inputs.3.borrow();
                self.outputs.0 = get_storage_slot_context(&context_ref, address, index);
            }
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "SloadNode: Load from address {} slot {} = {}",
                (*self.inputs.0).frame_input.target_address, *self.inputs.1, self.outputs.0
            )
        }
    }
}

// --- SSTORE Node (Simplified) ---

/// Node for SSTORE operation. Writes a storage slot. (Gas calculation ignored).
pub struct SstoreNode {
    inputs: StorageStoreInputs,
    outputs: U256Output,
}


impl SstoreNode {
    pub fn new(
        frame_context_ptr: *const FrameContext,
        index_ptr: *const U256,
        new_value_ptr: *const U256,
        context_ref: Rc<RefCell<ExternalContext>>,
    ) -> Self {
        Self {
            inputs: (frame_context_ptr, index_ptr, new_value_ptr, context_ref),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SstoreNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let new_value = *self.inputs.2;

            // Set the output value for dependent nodes in the graph
            self.outputs.0 = new_value;

            // Optional: Update the external context if this node is responsible for it.
            // if let Some(context_ref) = &self.inputs.3 {
            //     let address = u256_to_address!(*self.inputs.0);
            //     let index = *self.inputs.1;
            //     let mut context = context_ref.borrow_mut();
            //     // Here you might just insert/update the storage slot.
            //     // The more complex SStore logic (like refunds) is ignored.
            //     context.storage.insert((address, index), new_value);
            //     // You might also need logic to update AccountStatus if an account is touched.
            // }
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "SstoreNode: Store {} to address {} slot {}",
                *self.inputs.2, (*self.inputs.0).frame_input.target_address, *self.inputs.1
            )
        }
    }
}

// --- BALANCE Node ---

/// Node for BALANCE operation. Reads account balance.
pub struct BalanceNode {
    inputs: BalanceCheckInputs,
    outputs: U256Output,
}


impl BalanceNode {
    pub fn new(
        address_u256_ptr: *const U256,
        info_ptr: Option<*const AccountInfo>,
        context_ref: Rc<RefCell<ExternalContext>>,
    ) -> Self {
        Self {
            inputs: (address_u256_ptr, info_ptr, context_ref),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for BalanceNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let balance = if let Some(info_ptr) = self.inputs.1 {
                (*info_ptr).balance
            } else {
                let context_ref = &self.inputs.2.borrow();
                let address = u256_to_address!(*self.inputs.0); // Convert
                get_account_context(&context_ref, address).0.balance // Get AccountInfo part
            
            };
            self.outputs.0 = balance;
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "BalanceNode: Balance of address {} = {}",
                *self.inputs.0, self.outputs.0
            )
        }
    }
}

// --- EXTCODESIZE Node ---

/// Node for EXTCODESIZE operation. Reads external contract code size.
pub struct ExtcodesizeNode {
    inputs: CodeInfoInputs,
    outputs: U256Output,
}


impl ExtcodesizeNode {
    pub fn new(
        address_u256_ptr: *const U256,
        info_ptr: Option<*const AccountInfo>,
        context_ref: Rc<RefCell<ExternalContext>>,
    ) -> Self {
        Self {
            inputs: (address_u256_ptr, info_ptr, context_ref),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ExtcodesizeNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let code_len = if let Some(info_ptr) = self.inputs.1 {
                (*info_ptr).code.as_ref().map_or(0, |c| c.len())
            } else  {
                let context_ref = &self.inputs.2.borrow();
                let address = u256_to_address!(*self.inputs.0); // Convert
                get_account_context(&context_ref, address)
                    .0
                    .code
                    .as_ref()
                    .map_or(0, |c| c.len())
            };
            self.outputs.0 = U256::from(code_len);
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "ExtcodesizeNode: Code size of address {} = {}",
                *self.inputs.0, self.outputs.0
            )
        }
    }
}

// --- EXTCODEHASH Node ---

/// Node for EXTCODEHASH operation. Reads external contract code hash.
pub struct ExtcodehashNode {
    inputs: CodeInfoInputs,
    outputs: U256Output,
}


impl ExtcodehashNode {
    pub fn new(
        address_u256_ptr: *const U256,
        info_ptr: Option<*const AccountInfo>,
        context_ref: Rc<RefCell<ExternalContext>>,
    ) -> Self {
        Self {
            inputs: (address_u256_ptr, info_ptr, context_ref),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ExtcodehashNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let (info, status) = if let Some(info_ptr) = self.inputs.1 {
                ((*info_ptr).clone(), AccountStatus::Loaded) // Assume loaded if from graph
            } else {
                let context_ref = &self.inputs.2.borrow();
                let address = u256_to_address!(*self.inputs.0); // Convert
                get_account_context(&context_ref, address)
            };

            // Logic per EIP-1052: Hash is KECCAK_EMPTY if account doesn't exist or is empty.
            // AccountStatus::LoadedEmpty indicates non-existence or empty state from external source.
            let code_hash = if status.is_empty() || info.is_empty() {
                KECCAK_EMPTY
            } else {
                info.code_hash
            };

            self.outputs.0 = U256::from_be_bytes(code_hash.0);
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "ExtcodehashNode: Code hash of address {} = {}",
                *self.inputs.0, self.outputs.0
            )
        }
    }
}

// --- BLOCKHASH Node ---

/// Node for BLOCKHASH operation. Reads historical block hash.
pub struct BlockhashNode {
    inputs: BlockHashInputs,
    outputs: U256Output,
}


impl BlockhashNode {
    pub fn new(
        number_ptr: *const U256,
        context_ref: Rc<RefCell<ExternalContext>>,
        current_block_number_ptr: *const U256,
    ) -> Self {
        Self {
            inputs: (number_ptr, context_ref, current_block_number_ptr),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for BlockhashNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let number_u256 = *self.inputs.0;
            let current_block_number = *self.inputs.2;
            let context = self.inputs.1.borrow();

            let number = as_u64_saturated!(number_u256);
            let upper_bound = as_u64_saturated!(current_block_number);
            let lower_bound = upper_bound.saturating_sub(256);

            if number >= upper_bound || number < lower_bound {
                self.outputs.0 = U256::ZERO;
            } else {
                // Fetch from context's block_hashes
                match context.block_hashes.get(&number) {
                    Some(hash) => {
                        self.outputs.0 = U256::from_be_bytes(hash.0);
                    }
                    None => {
                        self.outputs.0 = U256::ZERO; // Return zero if not found
                    }
                }
            }
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "BlockhashNode: Hash of block {} = {}",
                *self.inputs.0, self.outputs.0
            )
        }
    }
}

// --- SELFBALANCE Node ---

/// Node for SELFBALANCE operation. Reads the balance of the current contract.
pub struct SelfBalanceNode {
    inputs: SelfBalanceInputs,
    outputs: U256Output,
}


impl SelfBalanceNode {
    pub fn new(
        frame_context_ptr: *const FrameContext,
        info_ptr: Option<*const AccountInfo>,
        context_ref: Rc<RefCell<ExternalContext>>,
    ) -> Self {
        Self {
            inputs: (frame_context_ptr, info_ptr, context_ref),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SelfBalanceNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let balance = if let Some(info_ptr) = self.inputs.1 {
                // Balance comes from a preceding node modifying the account info
                (*info_ptr).balance
            } else {
                let context_ref = &self.inputs.2.borrow();
                // Balance needs to be fetched from the external context
                let frame_context = &*self.inputs.0;
                let address = frame_context.frame_input.target_address; // Get self address
                get_account_context(&context_ref, address).0.balance // Fetch info and get balance
            };
            self.outputs.0 = balance;
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            let address = (*self.inputs.0).frame_input.target_address;
            format!(
                "SelfBalanceNode: Balance of self ({}) = {}",
                address, self.outputs.0
            )
        }
    }
}

// --- EXTCODECOPY Node ---

/// Node for EXTCODECOPY operation. Copies external contract bytecode to memory.
pub struct ExtcodecopyNode {
    inputs: ExtCodeCopyInputs,
    _outputs: (),
}


impl ExtcodecopyNode {
    pub fn new(
        address_u256_ptr: *const U256,
        mem_offset_ptr: *const U256,
        code_offset_ptr: *const U256,
        len_ptr: *const U256,
        info_ptr: Option<*const AccountInfo>,
        context_ref: Rc<RefCell<ExternalContext>>,
        memory: Rc<RefCell<SharedMemory>>,
    ) -> Self {
        Self {
            inputs: (
                address_u256_ptr,
                mem_offset_ptr,
                code_offset_ptr,
                len_ptr,
                info_ptr,
                context_ref,
                memory,
            ),
            _outputs: (),
        }
    }
}

impl TypedNode for ExtcodecopyNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let mem_offset = as_usize_saturated!(*self.inputs.1);
            let code_offset = as_usize_saturated!(*self.inputs.2);
            let len = as_usize_saturated!(*self.inputs.3);

            // Get the code bytes
            let code_bytes = if let Some(info_ptr) = self.inputs.4 {
                 // Code info comes from a preceding node
                 (*info_ptr).code.as_ref().map_or(Bytes::new(), |c| c.bytes().clone()) // Get original bytes
            } else {
                 let context_ref = &self.inputs.5.borrow();
                 // Code info needs to be fetched from the external context
                 let address = u256_to_address!(*self.inputs.0); // Convert address
                 get_account_context(&context_ref, address).0.code.as_ref().map_or(Bytes::new(), |c| c.bytes().clone())
            };


            // Borrow memory mutably
            let mut memory = self.inputs.6.borrow_mut();

            // Ensure memory is large enough for the write operation
            let required_mem_size = mem_offset.saturating_add(len);
             if required_mem_size > memory.len() {
                memory.resize(required_mem_size);
            }

            // Perform the copy logic (similar to revm-ssa-graph)
            if len > 0 {
                memory.set_data(mem_offset, code_offset, len, &code_bytes);
            }
        }
        Ok(())
    }

     fn print(&self) -> String {
        unsafe {
            format!(
                "ExtcodecopyNode: Copy from address {} (code offset {}) to memory offset {} (length {})",
                *self.inputs.0, *self.inputs.2, *self.inputs.1, *self.inputs.3
            )
        }
    }
}

// --- SELFDESTRUCT Node ---

/// Node for SELFDESTRUCT operation. Marks contract for deletion and transfers balance.
pub struct SelfdestructNode {
    inputs: SelfDestructInputs,
    outputs: SelfDestructOutputs,
}


impl SelfdestructNode {
    pub fn new(
        target_u256_ptr: *const U256,
        frame_context_ptr: *const FrameContext,
        self_info_ptr: Option<*const AccountInfo>,
        target_info_ptr: Option<*const AccountInfo>,
        self_status_ptr: Option<*const AccountStatus>,
        context_ref: Rc<RefCell<ExternalContext>>,
        is_cancun_enabled: bool,
    ) -> Self {
        Self {
            inputs: (
                target_u256_ptr,
                frame_context_ptr,
                self_info_ptr,
                target_info_ptr,
                self_status_ptr,
                context_ref,
                is_cancun_enabled,
            ),
            outputs: (
                AccountInfo::default(),
                AccountInfo::default(),
                AccountStatus::Loaded,
                InstructionResult::Stop,
            ),
        }
    }
}

impl TypedNode for SelfdestructNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let target_address = u256_to_address!(*self.inputs.0);
            let frame_context = &*self.inputs.1;
            let self_address = frame_context.frame_input.target_address;
            let is_cancun_enabled = self.inputs.6;
            let context = self.inputs.5.borrow();
            
            let mut self_info = self.inputs.2.map_or_else(
                || {
                    get_account_context(&context, self_address).0
                },
                |ptr| (*ptr).clone()
            );
            let self_status = self.inputs.4.map_or_else(
                || {
                    get_account_context(&context, self_address).1
                },
                |ptr| (*ptr)
            );

            let (mut target_info, _target_status) = self.inputs.3.map_or_else(
                || {
                    get_account_context(&context, target_address)
                },
                |ptr| ((*ptr).clone(), AccountStatus::Loaded), // Assume loaded if from graph
            );

            let self_balance = self_info.balance; // Cache balance before zeroing

            // Transfer balance if target is different
            if self_address != target_address {
                target_info.balance = target_info.balance.saturating_add(self_balance);
            }

            // Zero self balance
            self_info.balance = U256::ZERO;

            // Determine final status based on Cancun fork
            let is_created = self_status.contains(AccountStatus::Created);
            let final_self_status = if is_created || !is_cancun_enabled {
                 AccountStatus::Loaded | AccountStatus::SelfDestructed
            } else {
                 self_status
            };

            // Set outputs
            self.outputs.0 = self_info;
            self.outputs.1 = target_info;
            self.outputs.2 = final_self_status;
            self.outputs.3 = InstructionResult::SelfDestruct;
        }
        Ok(())
    }


    fn print(&self) -> String {
        unsafe {
            let target = u256_to_address!(*self.inputs.0);
            format!(
                "SelfdestructNode: Selfdestruct address {} (contract address {})",
                target, ((*self.inputs.1).frame_input.target_address)
            )
        }
    }

    fn get_account_info_output(&self, index: usize) -> Option<*const AccountInfo> {
        match index {
            0 => Some(&self.outputs.0),
            1 => Some(&self.outputs.1),
            _ => None,
        }
    }
    
    fn get_account_status_output(&self) -> *const AccountStatus {
        &self.outputs.2
    }

    fn get_instruction_result_output(&self) -> *const InstructionResult {
        &self.outputs.3
    }
    
}

// --- Remaining Nodes (TLOAD, TSTORE, LOG) ---
// These should be implemented following the same pattern:
// - Use pointers for graph-internal dependencies.
// - Use Rc<RefCell<ExternalContext>> for external state access.
// - Use u256_to_address macro where U256 inputs represent addresses.
// - TLOAD/TSTORE would likely need a separate HashMap in ExternalContext for transient storage.
// - SELFDESTRUCT would need careful handling of multiple state changes (updating account info/status in ExternalContext).
