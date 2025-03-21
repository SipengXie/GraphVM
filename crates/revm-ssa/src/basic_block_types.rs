#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BasicBlockId {
    // 合约代码哈希
    pub code_hash: B256,
    // 起始PC位置
    pub start_pc: usize,
    // 结束PC位置
    pub end_pc: usize,
}

// 我觉得我们不需要重构一个完整的Stack，Push0~Push32的结果我们仍然只需要是常数，我们只需要一个Non-constant stack（也就是input_stack）
// ========================== 记录阶段 ================================
// 见草稿纸


// 执行阶段优化
// 1. 将vec优化为数组
// 2. 优化SSALogEntry为Trait，用泛型对不同的I-O的声明类型
// 3. 忽略全常量输入的opcode