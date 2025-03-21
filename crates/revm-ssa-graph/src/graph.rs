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
    /// Mapping from LSN to node index, using Vec since LSN is sequential from 1..N
    lsn_to_node: Vec<NodeIndex>,
    /// Mapping from lsn to node index with storage write
    storage_write: Vec<LsnType>,
}


impl SsaGraph {
    pub fn new(node_num: usize, edge_num: usize) -> Self {
        Self {
            graph: DiGraph::with_capacity(node_num, edge_num),
            lsn_to_node: vec![NodeIndex::new(0); node_num + 1],
            storage_write: Vec::with_capacity(node_num + 1),
        }
    }

    #[inline(always)]
    pub fn num_nodes(&self) -> usize {
        self.lsn_to_node.len()
    }

    #[inline(always)]
    pub fn get_node_by_index(&self, index: NodeIndex) -> Result<&SSALogEntry> {
        Ok(&self.graph[index])
    }

    /// Add a node
    #[inline(always)]
    pub fn add_node(&mut self, entry: SSALogEntry) -> Result<()> {
        // eprintln!("entry: {}", entry);
        let lsn = entry.lsn;
        if is_storage_write!(entry.opcode) {
            self.storage_write.push(lsn);
        }
        let node_idx = self.graph.add_node(entry);
        
        //The vector has enough capacity for the current LSN
        self.lsn_to_node[lsn as usize] = node_idx;
        
        Ok(())
    }

    /// Get LSN dependencies from SSAInput
    #[inline(always)]
    pub fn get_lsn_from_input(input: &SSAInput) -> Vec<LsnType> {
        let mut lsn_vec = Vec::with_capacity(1);
        match input {
            SSAInput::Constant(_) => lsn_vec.push(0),
            SSAInput::Stack(lsn_with_index) => lsn_vec.push(lsn_with_index.0),
            SSAInput::Memory (source) => {
                if source.is_empty() {
                    lsn_vec.push(0)
                } else {
                    // Get LSN from the last memory dependency
                    // Memory may contains multiple dependencies
                    source.iter().for_each(|dep| lsn_vec.push(dep.lsn.0))
                }
            },
            SSAInput::Storage (_,source) => lsn_vec.push(source.0),
            SSAInput::ReturnDataBuffer (source) => lsn_vec.push(source.0),
            SSAInput::ContractEnv (entry_lsn) => {
                if entry_lsn.0 != 2 {
                    lsn_vec.push(entry_lsn.0) // we should consider the first contract_env(lsn:2) as a constant
                }
            },
            SSAInput::MemorySizeChange (last_memory) => lsn_vec.push(last_memory.0),
            SSAInput::CreateInput (source) => lsn_vec.push(source.0),
            SSAInput::CallInput (source) => lsn_vec.push(source.0),
            SSAInput::InterpreterResult (source) => lsn_vec.push(source.0),
            SSAInput::CallOutcome (source) => lsn_vec.push(source.0),
            SSAInput::CreateOutcome (source) => lsn_vec.push(source.0),
        };
        lsn_vec
    }

    /// Get a reference to execution results, primarily used by tracer
    /// 
    /// # Arguments
    /// * `lsn` - Logical sequence number
    /// 
    /// # Returns
    /// * `Result<Option<&[SSAOutput]>>` - A reference to execution results if found
    pub fn get_original_outputs(&self, lsn: LsnType) -> Result<Option<&[SSAOutput]>> {
        let node_idx = self.lsn_to_node[lsn as usize];
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
    #[inline(always)]
    pub fn get_result<T, F>(&self, lsn: LsnType, extractor: F) -> Result<Option<T>>
    where
        F: FnOnce(&[SSAOutput]) -> Option<T>,
    {          
        if lsn as usize >= self.lsn_to_node.len() {
            return Err(ExecutionError::GraphError(format!("Node not found for LSN: {}", lsn)));
        }
        
        let node_idx = self.lsn_to_node[lsn as usize];
        Ok(extractor(&self.graph[node_idx].outputs))
    }

    /// Add edges
    #[inline(always)]
    pub fn add_edges(&mut self, lsn: LsnType) -> Result<()> {
        let lsn = lsn as usize;
        if lsn >= self.lsn_to_node.len() {
            return Err(ExecutionError::GraphError(format!("Node not found for LSN: {}", lsn)));
        }
        
        let node_idx = self.lsn_to_node[lsn];

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
            if dep_lsn as usize >= self.lsn_to_node.len() {
                return Err(ExecutionError::GraphError(format!("Dependency node not found for LSN: {}", dep_lsn)));
            }
            dep_indices.insert(self.lsn_to_node[dep_lsn as usize]);
        }

        // Add all edges
        for dep_idx in dep_indices {
            self.graph.add_edge(dep_idx, node_idx, ());
        }

        Ok(())
    }

    /// Get topological sort
    #[inline(always)]
    pub fn topological_sort(&self) -> Result<Vec<NodeIndex>> {
        let sorted_indices = toposort(&self.graph, None)
            .map_err(|_| ExecutionError::GraphError("Cycle detected in dependency graph".to_string()))?;
            
        Ok(sorted_indices)
    }

    /// Get mutable node
    #[inline(always)]
    pub fn get_node_mut(&mut self, lsn: LsnType) -> Result<&mut SSALogEntry> {
        let lsn = lsn as usize;
        if lsn >= self.lsn_to_node.len() {
            return Err(ExecutionError::GraphError(format!("Node not found for LSN: {}", lsn)));
        }
        
        let node_idx = self.lsn_to_node[lsn];
        Ok(&mut self.graph[node_idx])
    }

    /// Get immutable node
    #[inline(always)]
    pub fn get_node(&self, lsn: LsnType) -> Result<&SSALogEntry> {
        let lsn = lsn as usize;
        if lsn >= self.lsn_to_node.len() {
            return Err(ExecutionError::GraphError(format!("Node not found for LSN: {}", lsn)));
        }
        
        let node_idx = self.lsn_to_node[lsn];
        Ok(&self.graph[node_idx])
    }

    /// Get all reachable nodes from a starting LSN in dependency order using BFS
    /// 
    /// # Arguments
    /// * `start_lsn` - The starting LSN to traverse from
    /// 
    /// # Returns
    /// * `Result<Vec<NodeIndex>>` - A vector of reachable node indices in dependency order
    #[inline(always)]
    pub fn get_reachable_nodes(&self, start_lsn: LsnType) -> Result<Vec<NodeIndex>> {
        let lsn = start_lsn as usize;
        // Get the starting node index
        if lsn >= self.lsn_to_node.len() {
            return Err(ExecutionError::GraphError(format!("Node not found for LSN: {}", start_lsn)));
        }
        
        let start_idx = self.lsn_to_node[lsn];
        
        // Collect all reachable node indices using BFS
        let mut bfs = petgraph::visit::Bfs::new(&self.graph, start_idx);
        let mut node_indices = Vec::new();
        
        while let Some(nx) = bfs.next(&self.graph) {
            node_indices.push(nx);
        }
        
        Ok(node_indices)
    }

    /// Get a mutable reference to a node by its index
    /// 
    /// # Arguments
    /// * `index` - The node index
    /// 
    /// # Returns
    /// * `&mut SSALogEntry` - A mutable reference to the node
    #[inline(always)]
    pub fn get_node_by_index_mut(&mut self, index: NodeIndex) -> &mut SSALogEntry {
        &mut self.graph[index]
    }

    /// Get all storage write outputs and their corresponding storage keys
    /// 
    /// # Returns
    /// * `Result<Vec<SSAOutput>>` - A vector of all storage write outputs
    #[inline(always)]
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
                                value: value.clone(),
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

    /// Calculate the parallelism ratio of the graph
    /// 
    /// This function computes the ratio of critical path length to total nodes,
    /// which indicates the potential for parallelism. A lower ratio means higher
    /// potential for parallelization.
    /// 
    /// # Returns
    /// * `Result<f64>` - The ratio of critical path length to total nodes
    pub fn calculate_parallelism_ratio(&self) -> Result<f64> {
        let total_nodes = self.num_nodes();
        if total_nodes == 0 {
            return Ok(0.0);
        }
        
        let mut longest_paths: HashMap<NodeIndex, usize> = HashMap::default();
        
        let sorted_indices = toposort(&self.graph, None)
            .map_err(|_| ExecutionError::GraphError("Cycle detected in dependency graph".to_string()))?;
        
        for &node_idx in &sorted_indices {
            let incoming_count = self.graph.neighbors_directed(node_idx, petgraph::Direction::Incoming).count();
            if incoming_count == 0 {
                longest_paths.insert(node_idx, 1);
            } else {
                longest_paths.entry(node_idx).or_insert(1);
            }
        }
        
        for &node_idx in &sorted_indices {
            let current_path_len = *longest_paths.get(&node_idx).unwrap();
            
            for succ in self.graph.neighbors_directed(node_idx, petgraph::Direction::Outgoing) {
                let entry = longest_paths.entry(succ).or_insert(1);
                *entry = (*entry).max(current_path_len + 1);
            }
        }
        
        let critical_path_length = longest_paths.values().max().copied().unwrap_or(1);
        Ok(critical_path_length as f64 / total_nodes as f64)
    }

    /// Get nodes grouped by execution layers (topological levels)
    /// 
    /// # Returns
    /// * `Result<Vec<Vec<SSALogEntry>>>` - Nodes grouped by layers where each layer can be executed in parallel
    #[inline(always)]
    pub fn execution_layers(&self) -> Result<Vec<Vec<SSALogEntry>>> {
        // Get topologically sorted node indices
        let sorted_indices = toposort(&self.graph, None)
            .map_err(|_| ExecutionError::GraphError("Cycle detected in dependency graph".to_string()))?;

        // Use array to store level information (more efficient than HashMap)
        let mut levels = vec![0; self.graph.node_count()];
        let mut max_level = 0;

        // First pass: calculate the level for each node
        for &node_idx in &sorted_indices {
            // Get the maximum level of all predecessor nodes
            let pred_level = self.graph.neighbors_directed(node_idx, petgraph::Direction::Incoming)
                .map(|p| levels[p.index()])
                .max()
                .unwrap_or(0);

            // Current node level = max predecessor level + 1
            let current_level = pred_level + 1;
            levels[node_idx.index()] = current_level;
            
            // Update the maximum level
            if current_level > max_level {
                max_level = current_level;
            }
        }

        // Second pass: group by levels (pre-allocate space for better performance)
        let mut layers: Vec<Vec<SSALogEntry>> = vec![Vec::new(); max_level as usize];
        for (_i, &node_idx) in sorted_indices.iter().enumerate() {
            let level = levels[node_idx.index()] - 1; // Levels start from 0
            layers[level].push(self.graph[node_idx].clone());
        }

        Ok(layers)
    }
}