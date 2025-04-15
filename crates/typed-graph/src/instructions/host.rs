use crate::{
    context::{get_account_context, get_storage_slot_context, ExternalContext},
    typed_graph::{HasInputType, HasOutputType, TypedNode},
    u256_to_address,
};
use revm_interpreter::as_u64_saturated;
use revm_primitives::{AccountInfo, AccountStatus, KECCAK_EMPTY, U256};
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

// --- Remaining Nodes (TLOAD, TSTORE, SELFBALANCE, EXTCODECOPY, LOG, SELFDESTRUCT) ---
// These should be implemented following the same pattern:
// - Use pointers for graph-internal dependencies.
// - Use Rc<RefCell<ExternalContext>> for external state access.
// - Use u256_to_address macro where U256 inputs represent addresses.
// - TLOAD/TSTORE would likely need a separate HashMap in ExternalContext for transient storage.
// - SELFDESTRUCT would need careful handling of multiple state changes (updating account info/status in ExternalContext).
