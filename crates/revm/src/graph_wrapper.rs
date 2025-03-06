use std::sync::Arc;

// use metrics::histogram;
use revm_ssa::{logger::LsnType, SSALogEntry};
use revm_ssa_graph::SsaGraph;

pub struct GraphWrapper {
    graph: Arc<SsaGraph>,
    is_built: bool,
}

impl GraphWrapper {
    pub fn new(node_capacity: usize, edge_capacity: usize) -> Self {
        GraphWrapper {
            graph: Arc::new(SsaGraph::new(node_capacity, edge_capacity)),
            is_built: false,
        }
    }

    pub fn build(&mut self, entries: Vec<SSALogEntry>) {
        if self.is_built {
            return;
        }

        let graph = Arc::get_mut(&mut self.graph)
            .expect("Arc should be unique during build");

        let lsns: Vec<LsnType> = entries.iter().map(|entry| entry.lsn).collect();

        for entry in entries {
            graph.add_node(entry).unwrap();
        }

        for lsn in lsns {
            match graph.add_edges(lsn) {
                Ok(_) => {},
                Err(e) => {
                    // Output current node and max LSN when error occurs
                    let node = graph.get_node(lsn);
                    let max_lsn = graph.num_nodes();
                    println!("Error adding edges for LSN {}: {:?}", lsn, e);
                    println!("Current node: {:?}", node);
                    println!("Max LSN: {}", max_lsn+1);
                    
                    // Output all nodes
                    for i in 1..max_lsn+1 {
                        let node = graph.get_node(i as LsnType).unwrap();
                        println!("Node {}: {:?}", i, node);
                    }
                    panic!("Execution Error: {:?}", e);
                }
            }
        }
        
        self.is_built = true;
    }

    pub fn get_graph(&self) -> Arc<SsaGraph> {
        self.graph.clone()
    }

    pub fn is_built(&self) -> bool {
        self.is_built
    }
}