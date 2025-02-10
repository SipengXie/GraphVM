use revm_ssa::SSAOutput;
use std::collections::HashMap;

/// Mismatch record
#[derive(Debug, Clone)]
pub struct MismatchRecord {
    pub lsn: usize,
    pub opcode: u8,
    pub original: Vec<SSAOutput>,
    pub graph: Vec<SSAOutput>,
}

/// Execution tracer
#[derive(Debug)]
pub struct ExecutionTracer {
    /// Original execution results
    original_results: HashMap<usize, Vec<SSAOutput>>,
    /// Mismatch records
    mismatches: Vec<MismatchRecord>,
}

impl ExecutionTracer {
    pub fn new() -> Self {
        Self {
            original_results: HashMap::new(),
            mismatches: Vec::new(),
        }
    }

    /// Record original execution result
    pub fn record_original(&mut self, lsn: usize, outputs: Vec<SSAOutput>) {
        self.original_results.insert(lsn, outputs);
    }

    /// Record graph execution result
    pub fn record_graph(&mut self, lsn: usize, outputs: Vec<SSAOutput>, opcode: u8) {
        // Check if results match
        if let Some(original) = self.original_results.get(&lsn) {
            if !Self::compare_results(original, &outputs) {
                self.mismatches.push(MismatchRecord {
                    lsn,
                    opcode,
                    original: original.clone(),
                    graph: outputs,
                });
            }
        }
    }

    /// Compare if two results are equal
    fn compare_results(original: &[SSAOutput], graph: &[SSAOutput]) -> bool {
        // Filter out MemorySize type outputs
        let original_filtered: Vec<_> = original.iter()
            .filter(|output| !matches!(output, SSAOutput::MemorySize(_)))
            .collect();
        let graph_filtered: Vec<_> = graph.iter()
            .filter(|output| !matches!(output, SSAOutput::MemorySize(_)))
            .collect();

        if original_filtered.len() != graph_filtered.len() {
            return false;
        }

        original_filtered.iter().zip(graph_filtered.iter())
            .all(|(a, b)| Self::compare_output(a, b))
    }

    /// Compare if two outputs are equal
    fn compare_output(a: &SSAOutput, b: &SSAOutput) -> bool {
        match (a, b) {
            // Compare stack outputs
            (SSAOutput::Stack(v1), SSAOutput::Stack(v2)) => v1 == v2,
            
            // Compare memory outputs
            (SSAOutput::Memory(v1), SSAOutput::Memory(v2)) => v1 == v2,
            
            // Compare storage outputs
            (SSAOutput::Storage { key: k1, value: v1 }, SSAOutput::Storage { key: k2, value: v2 }) => {
                k1 == k2 && v1 == v2
            },
            
            // Compare return data
            (SSAOutput::ReturnDataBuffer(v1), SSAOutput::ReturnDataBuffer(v2)) => v1 == v2,
            
            // Compare memory size
            (SSAOutput::MemorySize(s1), SSAOutput::MemorySize(s2)) => s1 == s2,
            
            // Compare addresses
            (SSAOutput::Address(a1), SSAOutput::Address(a2)) => a1 == a2,
            
            // Compare jumps
            (SSAOutput::Jump { relative_offset: o1 }, SSAOutput::Jump { relative_offset: o2 }) => o1 == o2,
            
            // Compare call frames
            (SSAOutput::CallFrame(f1), SSAOutput::CallFrame(f2)) => {
                f1.caller == f2.caller &&
                f1.target_address == f2.target_address &&
                f1.input == f2.input &&
                f1.value == f2.value &&
                f1.scheme == f2.scheme &&
                f1.ret_range == f2.ret_range
            },
            
            // Compare call outcomes
            (SSAOutput::CallOutcome(o1), SSAOutput::CallOutcome(o2)) => {
                o1.result == o2.result &&
                o1.ret_range == o2.ret_range
            },
            
            // Compare create frames
            (SSAOutput::CreateFrame(f1), SSAOutput::CreateFrame(f2)) => {
                f1.caller == f2.caller &&
                f1.value == f2.value &&
                f1.init_code == f2.init_code &&
                f1.scheme == f2.scheme &&
                f1.target == f2.target
            },
            
            // Compare create outcomes
            (SSAOutput::CreateOutcome(o1), SSAOutput::CreateOutcome(o2)) => {
                o1.result == o2.result &&
                o1.address == o2.address
            },
            
            // Compare logs
            (SSAOutput::Log(l1), SSAOutput::Log(l2)) => l1 == l2,
            
            // Compare interpreter results
            (SSAOutput::InterpreterResult(r1), SSAOutput::InterpreterResult(r2)) => {
                r1.result == r2.result && r1.output == r2.output
            },
            
            // Different types of outputs are considered unequal
            _ => false,
        }
    }

    /// Generate comparison report
    pub fn generate_report(&self) -> String {
        if self.mismatches.is_empty() {
            "All results match".to_string()
        } else {
            let mut report = String::new();
            report.push_str("Mismatches found:\n");
            
            // Create a mutable clone for sorting
            let mut sorted_mismatches = self.mismatches.clone();
            // Sort by LSN
            sorted_mismatches.sort_by_key(|m| m.lsn);
            
            for mismatch in &sorted_mismatches {
                report.push_str(&format!("LSN {}: opcode 0x{:02x}\n", mismatch.lsn, mismatch.opcode));
                report.push_str("Original:\n");
                for output in &mismatch.original {
                    report.push_str(&format!("  {:?}\n", output));
                }
                report.push_str("Graph:\n");
                for output in &mismatch.graph {
                    report.push_str(&format!("  {:?}\n", output));
                }
                report.push_str("\n");
            }
            
            report
        }
    }
} 