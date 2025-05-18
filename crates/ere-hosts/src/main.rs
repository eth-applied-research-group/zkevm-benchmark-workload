//! Binary for benchmarking different Ere compatible zkVMs

use std::{collections::HashMap, path::PathBuf};
use witness_generator::generate_stateless_witness;
use zkevm_metrics::WorkloadMetrics;
use zkvm_interface::{Compiler, Input, zkVM};

/// Main entry point for the host benchmarker
fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv::dotenv().ok();

    // one line per host: no inner-loop duplication
    benchmark::<ere_sp1::RV32_IM_SUCCINCT_ZKVM_ELF, ere_sp1::EreSP1>(
        "sp1",
        concat!(env!("CARGO_WORKSPACE_DIR"), "ere-guests/sp1"),
    )?;

    // TODO: Add more
    Ok(())
}

// TODO: Eventually move this into benchmark_runner
fn benchmark<C, V>(host_name: &str, guest_dir: &str) -> Result<(), Box<dyn std::error::Error>>
where
    C: Compiler,
    C::Error: std::error::Error + Send + Sync + 'static,
    V: zkVM<C>,
    V::Error: std::error::Error + Send + Sync + 'static,
{
    println!("Benchmarking `{}`…", host_name);

    let path = PathBuf::from(guest_dir);

    // Compile program and generate proving + verifying key
    let program = C::compile(&path)?;
    let zkvm = V::new(program);

    // Generate test corpus
    let corpuses = generate_stateless_witness::generate();

    // Iterate through test corpus and generate reports
    for bw in &corpuses {
        println!(" {} ({} blocks)", bw.name, bw.blocks_and_witnesses.len());

        let mut reports = Vec::new();
        for ci in &bw.blocks_and_witnesses {
            let mut stdin = Input::new();
            stdin.write(ci)?;
            stdin.write(&bw.network)?;

            let report = zkvm.execute(&stdin)?;
            let region_cycles: HashMap<_, _> = report.region_cycles.into_iter().collect();

            reports.push(WorkloadMetrics {
                name: format!("{}-{}", bw.name, ci.block.number),
                total_num_cycles: report.total_num_cycles,
                region_cycles,
            });
        }

        let out_path = format!(
            "{}/zkevm-metrics/{}/{}.json",
            env!("CARGO_WORKSPACE_DIR"),
            host_name,
            bw.name
        );
        WorkloadMetrics::to_path(out_path, &reports)?;
        println!("wrote {} reports", reports.len());
    }

    Ok(())
}
