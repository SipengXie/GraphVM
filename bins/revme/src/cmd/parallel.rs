mod parallel_runner;
pub use parallel_runner::TestError as Error;
use std::path::PathBuf;
use structopt::StructOpt;

use parallel_runner::{run_parallel, run_sequential, TestError};

#[derive(StructOpt, Debug)]
pub struct Cmd {
    #[structopt(short, long, parse(try_from_str), default_value = "true")]
    parallel: bool,

    test_file: PathBuf,

    #[structopt(short, long, default_value = "2")]
    num_of_threads: usize,

    #[structopt(long, parse(try_from_str), default_value = "false")]
    enable_ssa: bool,
    #[structopt(long, parse(try_from_str), default_value = "false")]
    enable_dep_graph: bool,
    #[structopt(long, parse(try_from_str), default_value = "false")]
    enable_prefetch: bool,
}

impl Cmd {
    /// Run statetest command.
    pub fn run(&self) -> Result<(), TestError> {
        if self.parallel {
            println!("========== Running in parallel mode ==========");
            run_parallel(
                self.num_of_threads,
                self.enable_ssa,
                self.enable_dep_graph,
                self.enable_prefetch,
                &self.test_file,
            )?;
        } else {
            println!("========== Running in sequential mode ==========");
            run_sequential(&self.test_file)?;
        }

        Ok(())
    }
}
