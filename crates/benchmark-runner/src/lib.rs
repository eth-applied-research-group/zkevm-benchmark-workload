use rayon::prelude::*;
use witness_generator::{generate_stateless_witness, BlocksAndWitnesses};
use zkevm_metrics::WorkloadMetrics;

pub fn run_benchmark<F>(elf_path: &'static [u8], metrics_path_prefix: &str, zkvm_executor: F)
where
    F: Fn(&BlocksAndWitnesses, &'static [u8]) -> Vec<WorkloadMetrics> + Send + Sync,
{
    let generated_corpuses = generate_stateless_witness::generate();

    generated_corpuses.into_par_iter().for_each(|bw| {
        println!("{} (num_blocks={})", bw.name, bw.blocks_and_witnesses.len());

        let reports = zkvm_executor(&bw, elf_path);

        WorkloadMetrics::to_path(
            format!(
                "{}/{}/{}/{}.json",
                env!("CARGO_WORKSPACE_DIR"),
                "zkevm-metrics",
                metrics_path_prefix,
                bw.name
            ),
            &reports,
        )
        .unwrap();

        println!(
            "Finished processing and saved metrics for corpus: {}. Number of reports: {}",
            bw.name,
            reports.len()
        );
        // dbg!(&reports);
    });
}
