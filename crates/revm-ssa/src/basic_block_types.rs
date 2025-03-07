#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BasicBlockId {
    // 合约代码哈希
    pub code_hash: B256,
    // 起始PC位置
    pub start_pc: usize,
    // 结束PC位置
    pub end_pc: usize,
}

// 与之前不同,首先我们的stack_constant记录的是当前需要的栈下标, 以帮助图执行引擎直接从input_stack里获取数据
// 其次, 我们需要引入memory_constant,记录的是当前需要的内存下标, 以帮助图执行引擎直接从input_memory里获取数据, 这导致目前的memory_dependency需要重新设计
// 原先,memory_dependency记录的是([self_offset..self_offset+len] is set by lsn's output[lsn_offset..lsn_offset+len])
// 现在需要额外混合一种log, [self_offset..self_offset+len] is set by [memory_addr+memeory_addr+len], 这时候lsn为0


// 在每一个basic_block中,我们需要清空shadow_stack和shadow_memory[实际上是全部清洗为0]
// 然后, 我们需要清空storage的依赖记录, 如first_read和latest_write
// 我们还需要清空last_return记录, 把return_buffer作为外部输入
// 我们还需要清空call_inputs, contract_env这些外部元素
// 以下都需要被清空,并且,在graph中如果为0, 我们需要从context中获取相关信息
    // latest_writes: HashMap<StorageKey, LsnType>,
    // first_reads: HashMap<StorageKey, LsnType>,
    // last_memory: LsnType,
    // last_return_data_buffer: LsnType,
    // last_interpreter_return: LsnType,
    // last_sub_call: Vec<LsnType>,
    // last_sub_create: Vec<LsnType>,
    // last_call_return: Vec<LsnType>,
    // last_create_return: Vec<LsnType>
    // pub contract_env: Vec<LsnType>,
    // pub call_inputs: Vec<SSACallInput>,
