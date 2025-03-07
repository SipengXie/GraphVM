// use dashmap::DashMap;
// use metrics::histogram;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::algo::toposort;
use revm_ssa::logger::LsnType;
use revm_primitives::{HashMap, HashSet};
use crate::{Result, ExecutionError};
use revm_ssa::{SSALogEntry, SSAInput, SSAOutput};

/// Dependency graph
pub struct SsaGraph {
    /// Graph structure
    graph: DiGraph<SSALogEntry, ()>,
    /// Mapping from LSN to node index
    lsn_to_node: HashMap<LsnType, NodeIndex>,
    /// Mapping from node index to results
    results: HashMap<LsnType, Vec<SSAOutput>>,
    /// Mapping from lsn to node index with storage write
    storage_write: Vec<LsnType>,
}


impl SsaGraph {
    pub fn new(node_num: usize, edge_num: usize) -> Self {
        Self {
            graph: DiGraph::with_capacity(node_num, edge_num),
            lsn_to_node: HashMap::default(),
            results: HashMap::default(),
            storage_write: Vec::new(),
        }
    }

    pub fn num_nodes(&self) -> usize {
        self.lsn_to_node.len()
    }

    /// Add a node
    pub fn add_node(&mut self, entry: SSALogEntry) -> Result<()> {
        // eprintln!("entry: {}", entry);
        let lsn = entry.lsn;
        if is_storage_write!(entry.opcode) {
            // eprintln!("write_opcode:{}", lsn);
            self.storage_write.push(lsn);
        }
        let node_idx = self.graph.add_node(entry);
        self.lsn_to_node.insert(lsn, node_idx);
        Ok(())
    }

    /// Get LSN dependencies from SSAInput

    pub fn get_lsn_from_input(input: &SSAInput) -> Vec<LsnType> {
        let mut lsn_vec = Vec::with_capacity(1);
        match input {
            SSAInput::Constant(_) => lsn_vec.push(0),
            SSAInput::Stack { source, .. } => lsn_vec.push(*source),
            SSAInput::Memory { source, .. } => {
                if source.is_empty() {
                    lsn_vec.push(0)
                } else {
                    // Get LSN from the last memory dependency
                    // Memory may contains multiple dependencies
                    source.iter().for_each(|dep| lsn_vec.push(dep.lsn))
                }
            },
            SSAInput::Storage { source, .. } => lsn_vec.push(*source),
            SSAInput::ReturnDataBuffer { source, .. } => lsn_vec.push(*source),
            SSAInput::ContractEnv { source: entry_lsn, .. } => {
                if *entry_lsn != 2 {
                    lsn_vec.push(*entry_lsn) // we should consider the first contract_env(lsn:2) as a constant
                }
            },
            SSAInput::MemorySizeChange { source: last_memory, .. } => lsn_vec.push(*last_memory),
            SSAInput::CreateInput { source, .. } => lsn_vec.push(*source),
            SSAInput::CallInput { source, .. } => lsn_vec.push(*source),
            SSAInput::InterpreterResult { source, .. } => lsn_vec.push(*source),
            SSAInput::CallOutcome { source, .. } => lsn_vec.push(*source),
            SSAInput::CreateOutcome { source, .. } => lsn_vec.push(*source),
        };
        lsn_vec
    }

    /// Set execution result for a node
    pub fn set_result(&mut self, lsn: LsnType, outputs: Vec<SSAOutput>) -> Result<()> {
        // Directly modify node outputs
        self.results.insert(lsn, outputs);
        Ok(())
    }

    /// Get a reference to execution results, primarily used by tracer
    /// 
    /// # Arguments
    /// * `lsn` - Logical sequence number
    /// 
    /// # Returns
    /// * `Result<Option<&[SSAOutput]>>` - A reference to execution results if found
    pub fn get_original_outputs(&self, lsn: LsnType) -> Result<Option<&[SSAOutput]>> {
        let node_idx = *self.lsn_to_node.get(&lsn).ok_or_else(|| 
            ExecutionError::GraphError(format!("Node not found for LSN: {}", lsn))
        )?;
        Ok(Some(&self.graph[node_idx].outputs))
    }

    /// Get and extract a specific type of result using an extractor function
    /// 
    /// # Arguments
    /// * `lsn` - Logical sequence number
    /// * `extractor` - Function to extract the desired type from results
    /// 
    /// # Type Parameters
    /// * `T` - The type to extract
    /// * `F` - The extractor function type
    /// 
    /// # Returns
    /// * `Result<Option<T>>` - The extracted result if found
    pub fn get_result<T, F>(&self, lsn: LsnType, extractor: F) -> Result<Option<T>>
    where
        F: FnOnce(&[SSAOutput]) -> Option<T>,
    {       
        if let Some(outputs) = self.results.get(&lsn) {
            return Ok(extractor(outputs));
        }
        let node_idx = *self.lsn_to_node.get(&lsn).ok_or_else(|| 
            ExecutionError::GraphError(format!("Node not found for LSN: {}", lsn))
        )?;
        Ok(extractor(&self.graph[node_idx].outputs))
    }

    pub fn get_result_by_lsn(&self, lsn: LsnType) -> Result<Option<&Vec<SSAOutput>>> {
        if let Some(outputs) = self.results.get(&lsn) {
            return Ok(Some(outputs));
        }
        let node_idx = *self.lsn_to_node.get(&lsn).ok_or_else(|| 
            ExecutionError::GraphError(format!("Node not found for LSN: {}", lsn))
        )?;
        Ok(Some(&self.graph[node_idx].outputs))
    }

    /// Add edges
    pub fn add_edges(&mut self, lsn: LsnType) -> Result<()> {
        let node_idx = *self.lsn_to_node.get(&lsn).ok_or_else(|| 
            ExecutionError::GraphError(format!("Node not found for LSN: {}", lsn))
        )?;

        // Use HashSet to collect all LSN dependencies, automatically deduplicating
        let mut dep_lsns = HashSet::new();
        let entry = &self.graph[node_idx];

        // Collect LSNs from all inputs
        for input in &entry.inputs {
            let deps = Self::get_lsn_from_input(input);
            for dep_lsn in deps {
                if dep_lsn != entry.lsn && dep_lsn != 0 {
                    dep_lsns.insert(dep_lsn);
                }
            }
        }

        // Convert all LSNs to NodeIndex at once
        let mut dep_indices = HashSet::with_capacity(dep_lsns.len());
        for dep_lsn in dep_lsns {
            let dep_idx = self.lsn_to_node.get(&dep_lsn).ok_or_else(|| 
                ExecutionError::GraphError(format!("Dependency node not found for LSN: {}", dep_lsn))
            )?;
            dep_indices.insert(*dep_idx);
        }

        // Add all edges
        for dep_idx in dep_indices {
            self.graph.add_edge(dep_idx, node_idx, ());
        }

        Ok(())
    }

    /// Get topological sort
    pub fn topological_sort(&self) -> Result<Vec<SSALogEntry>> {
        let sorted_indices = toposort(&self.graph, None)
            .map_err(|_| ExecutionError::GraphError("Cycle detected in dependency graph".to_string()))?;
            
        Ok(sorted_indices.iter().map(|&idx| self.graph[idx].clone()).collect())
    }

    /// Get mutable node
    pub fn get_node_mut(&mut self, lsn: LsnType) -> Result<&mut SSALogEntry> {
        let node_idx = *self.lsn_to_node.get(&lsn).ok_or_else(|| 
            ExecutionError::GraphError(format!("Node not found for LSN: {}", lsn))
        )?;
        Ok(&mut self.graph[node_idx])
    }

    /// Get immutable node
    pub fn get_node(&self, lsn: LsnType) -> Result<&SSALogEntry> {
        let node_idx = *self.lsn_to_node.get(&lsn).ok_or_else(|| 
            ExecutionError::GraphError(format!("Node not found for LSN: {}", lsn))
        )?;
        Ok(&self.graph[node_idx])
    }

    /// Get all reachable nodes from a starting LSN in dependency order using BFS
    /// 
    /// # Arguments
    /// * `start_lsn` - The starting LSN to traverse from
    /// 
    /// # Returns
    /// * `Result<Vec<SSALogEntry>>` - A vector of reachable nodes in dependency order
    pub fn get_reachable_nodes(&self, start_lsn: LsnType) -> Result<Vec<SSALogEntry>> {
        // Get the starting node index
        let start_idx = *self.lsn_to_node.get(&start_lsn).ok_or_else(|| 
            ExecutionError::GraphError(format!("Node not found for LSN: {}", start_lsn))
        )?;
        
        // Use BFS to find all reachable nodes in order
        let mut bfs = petgraph::visit::Bfs::new(&self.graph, start_idx);
        let mut result = Vec::new();
        
        while let Some(nx) = bfs.next(&self.graph) {
            result.push(self.graph[nx].clone());
        }
        
        Ok(result)
    }

    /// Get all LSNs in the graph
    /// 
    /// # Returns
    /// * `Vec<LsnType>` - A vector of all LSNs in the graph
    pub fn get_lsns(&self) -> Vec<LsnType> {
        self.lsn_to_node.iter().map(|(lsn, _)| *lsn).collect()
    }

    /// Get all storage write outputs and their corresponding storage keys
    /// 
    /// # Returns
    /// * `Result<Vec<SSAOutput>>` - A vector of all storage write outputs
    pub fn get_storage_write_outputs(&self) -> Result<Vec<SSAOutput>> {
        // Pre-allocate capacity to avoid reallocations
        let mut storage_outputs = Vec::with_capacity(self.storage_write.len());

        // Get all results at once to avoid multiple get_result calls
        for lsn in &self.storage_write {
            if let Some(outputs) = self.get_result(*lsn, |outputs| {
                // Operate directly on iterator to avoid creating intermediate Vec
                outputs.iter()
                    .filter_map(|o| {
                        if let SSAOutput::Storage { key, value } = o {
                            Some(SSAOutput::Storage {
                                key: key.clone(),
                                value: value.clone()
                            })
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .into()
            })? {
                storage_outputs.extend(outputs);
            }
        }

        Ok(storage_outputs)
    }

}