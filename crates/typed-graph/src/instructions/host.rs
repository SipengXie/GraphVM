use crate::instructions::memory::calc_memory_size;
use crate::{
    context::{get_account_context, get_storage_slot_context, ExternalContext, FrameContext},
    typed_graph::{HasInputType, HasOutputType, TypedNode},
    u256_to_address,
};
use revm_interpreter::{as_u64_saturated, as_usize_saturated, InstructionResult, SharedMemory};
use revm_primitives::{AccountInfo, AccountStatus, Bytes, KECCAK_EMPTY, U256};
use std::{cell::RefCell, rc::Rc};

// --- SLOAD Node ---

/// Node for SLOAD operation. Reads a storage slot.
pub struct SloadNode {
    /// Inputs:
    /// 0: *const U256 - Contract address (as U256).
    /// 1: *const U256 - Storage slot index.
    /// 2: Option<*const U256> - Pointer to value if written by a previous node.
    /// 3: Option<Rc<RefCell<ExternalContext>>> - Ref to external context if value from DB.
    inputs: (
        *const U256,
        *const U256,
        Option<*const U256>,
        Option<Rc<RefCell<ExternalContext>>>,
    ),
    /// Output:
    /// 0: U256 - The value read from storage.
    outputs: (U256,),
}

// Update Input/Output Traits
impl
    HasInputType<(
        *const U256,
        *const U256,
        Option<*const U256>,
        Option<Rc<RefCell<ExternalContext>>>,
    )> for SloadNode
{
}
impl HasOutputType<(U256,)> for SloadNode {}

impl SloadNode {
    pub fn new(
        address_u256_ptr: *const U256, // Changed parameter name
        index_ptr: *const U256,
        value_ptr: Option<*const U256>,
        context_ref: Option<Rc<RefCell<ExternalContext>>>, // Changed parameter name
    ) -> Self {
        assert!(
            value_ptr.is_some() ^ context_ref.is_some(),
            "SLOAD must have exactly one value source"
        );
        Self {
            inputs: (address_u256_ptr, index_ptr, value_ptr, context_ref),
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
            } else if let Some(context_ref) = &self.inputs.3 {
                // Value comes from external context
                let address = u256_to_address!(*self.inputs.0); // Convert U256 to Address
                let index = *self.inputs.1;
                let context = context_ref.borrow();
                self.outputs.0 = get_storage_slot_context(&context, address, index);
            } else {
                unreachable!("SLOAD node created without a value source");
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
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

// --- SSTORE Node (Simplified) ---

/// Node for SSTORE operation. Writes a storage slot. (Gas calculation ignored).
pub struct SstoreNode {
    /// Inputs:
    /// 0: *const U256 - Contract address (as U256).
    /// 1: *const U256 - Storage slot index.
    /// 2: *const U256 - Value to store.
    /// 3: Option<Rc<RefCell<ExternalContext>>> - Ref to external context (optional, for potential state update).
    inputs: (
        *const U256,                          // address_u256_ptr
        *const U256,                          // index_ptr
        *const U256,                          // new_value_ptr
        Option<Rc<RefCell<ExternalContext>>>, // context_ref
    ),
    /// Output:
    /// 0: U256 - The new value written to storage (available for subsequent SLOADs).
    outputs: (U256,),
}

// Update Input/Output Traits
impl
    HasInputType<(
        *const U256,
        *const U256,
        *const U256,
        Option<Rc<RefCell<ExternalContext>>>,
    )> for SstoreNode
{
}
impl HasOutputType<(U256,)> for SstoreNode {}

impl SstoreNode {
    pub fn new(
        address_u256_ptr: *const U256,
        index_ptr: *const U256,
        new_value_ptr: *const U256,
        context_ref: Option<Rc<RefCell<ExternalContext>>>,
    ) -> Self {
        Self {
            inputs: (address_u256_ptr, index_ptr, new_value_ptr, context_ref),
            outputs: (U256::ZERO,), // Will be overwritten in execute
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
                *self.inputs.2, *self.inputs.0, *self.inputs.1
            )
        }
    }
}

// --- BALANCE Node ---

/// Node for BALANCE operation. Reads account balance.
pub struct BalanceNode {
    /// Inputs:
    /// 0: *const U256 - Address to check balance of (as U256).
    /// 1: Option<*const AccountInfo> - Pointer if info comes from preceding node.
    /// 2: Option<Rc<RefCell<ExternalContext>>> - Reference to external context.
    inputs: (
        *const U256,
        Option<*const AccountInfo>,
        Option<Rc<RefCell<ExternalContext>>>,
    ),
    outputs: (U256,),
}

impl
    HasInputType<(
        *const U256,
        Option<*const AccountInfo>,
        Option<Rc<RefCell<ExternalContext>>>,
    )> for BalanceNode
{
}
impl HasOutputType<(U256,)> for BalanceNode {}

impl BalanceNode {
    pub fn new(
        address_u256_ptr: *const U256, // Changed parameter name
        info_ptr: Option<*const AccountInfo>,
        context_ref: Option<Rc<RefCell<ExternalContext>>>, // Changed parameter name
    ) -> Self {
        assert!(
            info_ptr.is_some() ^ context_ref.is_some(),
            "BALANCE must have exactly one info source"
        );
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
            } else if let Some(context_ref) = &self.inputs.2 {
                let address = u256_to_address!(*self.inputs.0); // Convert
                let context = context_ref.borrow();
                get_account_context(&context, address).0.balance // Get AccountInfo part
            } else {
                unreachable!("BALANCE node created without an info source");
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
    inputs: (
        *const U256,
        Option<*const AccountInfo>,
        Option<Rc<RefCell<ExternalContext>>>,
    ),
    outputs: (U256,),
}

impl
    HasInputType<(
        *const U256,
        Option<*const AccountInfo>,
        Option<Rc<RefCell<ExternalContext>>>,
    )> for ExtcodesizeNode
{
}
impl HasOutputType<(U256,)> for ExtcodesizeNode {}

impl ExtcodesizeNode {
    pub fn new(
        address_u256_ptr: *const U256, // Changed name
        info_ptr: Option<*const AccountInfo>,
        context_ref: Option<Rc<RefCell<ExternalContext>>>, // Changed name
    ) -> Self {
        assert!(
            info_ptr.is_some() ^ context_ref.is_some(),
            "EXTCODESIZE must have exactly one info source"
        );
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
            } else if let Some(context_ref) = &self.inputs.2 {
                let address = u256_to_address!(*self.inputs.0); // Convert
                let context = context_ref.borrow();
                get_account_context(&context, address)
                    .0
                    .code
                    .as_ref()
                    .map_or(0, |c| c.len())
            } else {
                unreachable!("EXTCODESIZE node created without an info source");
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
    inputs: (
        *const U256,
        Option<*const AccountInfo>,
        Option<Rc<RefCell<ExternalContext>>>,
    ),
    outputs: (U256,),
}

impl
    HasInputType<(
        *const U256,
        Option<*const AccountInfo>,
        Option<Rc<RefCell<ExternalContext>>>,
    )> for ExtcodehashNode
{
}
impl HasOutputType<(U256,)> for ExtcodehashNode {}

impl ExtcodehashNode {
    pub fn new(
        address_u256_ptr: *const U256, // Changed name
        info_ptr: Option<*const AccountInfo>,
        context_ref: Option<Rc<RefCell<ExternalContext>>>, // Changed name
    ) -> Self {
        assert!(
            info_ptr.is_some() ^ context_ref.is_some(),
            "EXTCODEHASH must have exactly one info source"
        );
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
            } else if let Some(context_ref) = &self.inputs.2 {
                let address = u256_to_address!(*self.inputs.0); // Convert
                let context = context_ref.borrow();
                get_account_context(&context, address)
            } else {
                unreachable!("EXTCODEHASH node created without an info source");
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
    /// Inputs:
    /// 0: *const U256 - Block number.
    /// 1: Rc<RefCell<ExternalContext>> - Reference to external context.
    /// 2: *const U256 - Current block number (needed for validation).
    inputs: (*const U256, Rc<RefCell<ExternalContext>>, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, Rc<RefCell<ExternalContext>>, *const U256)> for BlockhashNode {}
impl HasOutputType<(U256,)> for BlockhashNode {}

impl BlockhashNode {
    pub fn new(
        number_ptr: *const U256,
        context_ref: Rc<RefCell<ExternalContext>>, // Changed name
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
    /// Inputs:
    /// 0: *const FrameContext - To get the current contract address.
    /// 1: Option<*const AccountInfo> - Pointer if info comes from preceding node.
    /// 2: Option<Rc<RefCell<ExternalContext>>> - Reference to external context.
    inputs: (
        *const FrameContext,
        Option<*const AccountInfo>,
        Option<Rc<RefCell<ExternalContext>>>,
    ),
    outputs: (U256,),
}

impl
    HasInputType<(
        *const FrameContext,
        Option<*const AccountInfo>,
        Option<Rc<RefCell<ExternalContext>>>,
    )> for SelfBalanceNode
{
}
impl HasOutputType<(U256,)> for SelfBalanceNode {}

impl SelfBalanceNode {
    pub fn new(
        frame_context_ptr: *const FrameContext,
        info_ptr: Option<*const AccountInfo>,
        context_ref: Option<Rc<RefCell<ExternalContext>>>,
    ) -> Self {
        // Ensure exactly one source for the account info/balance is provided
        assert!(
            info_ptr.is_some() ^ context_ref.is_some(),
            "SELFBALANCE must have exactly one info source"
        );
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
            } else if let Some(context_ref) = &self.inputs.2 {
                // Balance needs to be fetched from the external context
                let frame_context = &*self.inputs.0;
                let address = frame_context.frame_input.target_address; // Get self address
                let context = context_ref.borrow();
                get_account_context(&context, address).0.balance // Fetch info and get balance
            } else {
                // This case should be prevented by the assert in new()
                unreachable!("SELFBALANCE node created without an info source");
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
    /// Inputs:
    /// 0: *const U256 - Address of the external contract (as U256).
    /// 1: *const U256 - Memory offset to write to.
    /// 2: *const U256 - Code offset to read from.
    /// 3: *const U256 - Length of bytes to copy.
    /// 4: Option<*const AccountInfo> - Pointer if account info comes from preceding node.
    /// 5: Option<Rc<RefCell<ExternalContext>>> - Reference to external context.
    /// 6: Rc<RefCell<SharedMemory>> - Shared memory reference.
    inputs: (
        *const U256,
        *const U256,
        *const U256,
        *const U256,
        Option<*const AccountInfo>,
        Option<Rc<RefCell<ExternalContext>>>,
        Rc<RefCell<SharedMemory>>,
    ),
    /// Outputs: None directly, modifies memory.
    _outputs: (),
}

// Define the specific input type tuple
type ExtcodecopyInput = (
    *const U256,
    *const U256,
    *const U256,
    *const U256,
    Option<*const AccountInfo>,
    Option<Rc<RefCell<ExternalContext>>>,
    Rc<RefCell<SharedMemory>>,
);

impl HasInputType<ExtcodecopyInput> for ExtcodecopyNode {}
impl HasOutputType<()> for ExtcodecopyNode {} // No direct output value

impl ExtcodecopyNode {
    pub fn new(
        address_u256_ptr: *const U256,
        mem_offset_ptr: *const U256,
        code_offset_ptr: *const U256,
        len_ptr: *const U256,
        info_ptr: Option<*const AccountInfo>,
        context_ref: Option<Rc<RefCell<ExternalContext>>>,
        memory: Rc<RefCell<SharedMemory>>,
    ) -> Self {
        // Ensure exactly one source for the account info/code is provided
        assert!(
            info_ptr.is_some() ^ context_ref.is_some(),
            "EXTCODECOPY must have exactly one info source"
        );
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
            _outputs: (), // No direct output value
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
            } else if let Some(context_ref) = &self.inputs.5 {
                 // Code info needs to be fetched from the external context
                 let address = u256_to_address!(*self.inputs.0); // Convert address
                 let context = context_ref.borrow();
                 get_account_context(&context, address).0.code.as_ref().map_or(Bytes::new(), |c| c.bytes().clone())
            } else {
                 unreachable!("EXTCODECOPY node created without an info source");
            };


            // Borrow memory mutably
            let mut memory = self.inputs.6.borrow_mut();

            // Ensure memory is large enough for the write operation
            let required_mem_size = calc_memory_size(mem_offset, len);
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
    /// Inputs:
    /// 0: *const U256 - Target address for refund (as U256).
    /// 1: *const FrameContext - Current frame context (for self address).
    /// 2: Option<*const AccountInfo> - Pointer if self info comes from preceding node.
    /// 3: Option<*const AccountInfo> - Pointer if target info comes from preceding node.
    /// 4: Option<Rc<RefCell<ExternalContext>>> - Reference to external context (fallback).
    /// 5: *const Env - Environment to check spec features (e.g., Cancun).
    inputs: (
        *const U256,
        *const FrameContext,
        Option<*const AccountInfo>, // Self info source
        Option<*const AccountInfo>, // Target info source
        Option<Rc<RefCell<ExternalContext>>>,
        bool, // Cancun enabled
    ),
    /// Outputs:
    /// 0: AccountInfo - Updated self account info (balance zeroed).
    /// 1: AccountInfo - Updated target account info (balance increased).
    /// 2: AccountStatus - Final status of the self-destructed account.
    /// 3: InstructionResult - Always Ok for SELFDESTRUCT itself.
    outputs: (AccountInfo, AccountInfo, AccountStatus, InstructionResult),
}

// Define the specific input type tuple
type SelfdestructInput = (
    *const U256,
    *const FrameContext,
    Option<*const AccountInfo>,
    Option<*const AccountInfo>,
    Option<Rc<RefCell<ExternalContext>>>,
    bool, // Cancun enabled
);

impl HasInputType<SelfdestructInput> for SelfdestructNode {}
impl HasOutputType<(AccountInfo, AccountInfo, AccountStatus, InstructionResult)>
    for SelfdestructNode
{
}

impl SelfdestructNode {
    pub fn new(
        target_u256_ptr: *const U256,
        frame_context_ptr: *const FrameContext,
        self_info_ptr: Option<*const AccountInfo>,
        target_info_ptr: Option<*const AccountInfo>,
        context_ref: Option<Rc<RefCell<ExternalContext>>>,
        is_cancun_enabled: bool,
    ) -> Self {
        // Assert that at least one source exists if context is None
        assert!(
            context_ref.is_some() || (self_info_ptr.is_some() && target_info_ptr.is_some()),
            "SELFDESTRUCT needs context or direct info pointers"
        );
        Self {
            inputs: (
                target_u256_ptr,
                frame_context_ptr,
                self_info_ptr,
                target_info_ptr,
                context_ref,
                is_cancun_enabled,
            ),
            outputs: (
                AccountInfo::default(),
                AccountInfo::default(),
                AccountStatus::Loaded, // Initial status, will be updated
                InstructionResult::Stop, // Final result
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
            let is_cancun_enabled = self.inputs.5;

            // Get self and target account info, prioritizing direct pointers over context
            let (mut self_info, self_status) = self.inputs.2.map_or_else(
                || {
                    let context = self.inputs.4.as_ref().expect("Context missing for self_info").borrow();
                    get_account_context(&context, self_address)
                },
                |ptr| ((*ptr).clone(), AccountStatus::Loaded), // Assume loaded if from graph
            );

            let (mut target_info, _target_status) = self.inputs.3.map_or_else(
                || {
                    let context = self.inputs.4.as_ref().expect("Context missing for target_info").borrow();
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
}

// --- Remaining Nodes (TLOAD, TSTORE, LOG) ---
// These should be implemented following the same pattern:
// - Use pointers for graph-internal dependencies.
// - Use Rc<RefCell<ExternalContext>> for external state access.
// - Use u256_to_address macro where U256 inputs represent addresses.
// - TLOAD/TSTORE would likely need a separate HashMap in ExternalContext for transient storage.
// - SELFDESTRUCT would need careful handling of multiple state changes (updating account info/status in ExternalContext).
