use crate::task::Task;
/// Task Dependency Graph (DAG) implementation
///
/// This module provides a directed acyclic graph structure to track dependencies
/// between EVM transactions. The graph is used to:
/// - Represent execution order constraints
/// - Track data dependencies between transactions
/// - Optimize parallel execution scheduling
///
/// The implementation uses the `daggy` crate for the core graph operations and
/// maintains a mapping between task IDs and graph nodes.
use daggy::{Dag, NodeIndex, Walker};
use std::collections::HashMap;

/// Represents a directed acyclic graph of task dependencies
///
/// The structure combines:
/// - A DAG storing the actual dependency relationships
/// - A mapping from task IDs to graph nodes for efficient lookups
pub struct TaskDag {
    /// The underlying graph structure
    /// Uses unit types for both nodes and edges as we only care about topology
    dag: Dag<(), ()>,

    /// Maps task IDs to their corresponding node indices in the graph
    /// Enables O(1) lookup of a task's position in the graph
    task_to_node: HashMap<i32, NodeIndex>,
}

impl TaskDag {
    /// Creates a new empty TaskDag
    ///
    /// Initializes both the graph and the ID mapping with no elements
    pub fn new() -> Self {
        TaskDag {
            dag: Dag::new(),
            task_to_node: HashMap::new(),
        }
    }

    /// Adds a new task to the graph
    ///
    /// Creates a new node in the graph for the task and records its
    /// position in the ID mapping.
    ///
    /// # Parameters
    /// * `task` - Reference to the task to add
    ///
    /// # Returns
    /// * `NodeIndex` - Index of the newly created node
    pub fn add_task(&mut self, task: &Task) -> NodeIndex {
        let node = self.dag.add_node(());
        self.task_to_node.insert(task.tid, node);
        node
    }

    /// Adds a dependency edge between two tasks
    ///
    /// Creates a directed edge in the graph indicating that one task
    /// depends on another. The edge direction is from the dependency
    /// to the dependent task.
    ///
    /// # Parameters
    /// * `dependent` - The task that depends on another
    /// * `dependency` - The task that must complete first
    pub fn add_dependency(&mut self, dependent: &Task, dependency: &Task) {
        if let (Some(&dep_node), Some(&task_node)) = (
            self.task_to_node.get(&dependency.tid),
            self.task_to_node.get(&dependent.tid),
        ) {
            let _ = self.dag.add_edge(dep_node, task_node, ());
        }
    }

    /// Gets all dependencies of a given task
    ///
    /// Retrieves the nodes representing all tasks that the given task
    /// directly depends on (immediate predecessors in the graph).
    ///
    /// # Parameters
    /// * `task` - Reference to the task to find dependencies for
    ///
    /// # Returns
    /// * `Vec<NodeIndex>` - Vector of node indices for all dependencies
    pub fn get_dependencies(&self, task: &Task) -> Vec<NodeIndex> {
        if let Some(&node) = self.task_to_node.get(&task.tid) {
            self.dag
                .parents(node)
                .iter(&self.dag)
                .map(|(_, n)| n)
                .collect()
        } else {
            Vec::new()
        }
    }

    /// Looks up the task ID associated with a graph node
    ///
    /// Performs a reverse lookup in the ID mapping to find which task
    /// corresponds to a given node in the graph.
    ///
    /// # Parameters
    /// * `node` - The node index to look up
    ///
    /// # Returns
    /// * `Option<i32>` - The task ID if found, None if the node isn't mapped
    pub fn get_task_tid(&self, node: NodeIndex) -> Option<i32> {
        self.task_to_node
            .iter()
            .find_map(|(&tid, &n)| if n == node { Some(tid) } else { None })
    }
}
