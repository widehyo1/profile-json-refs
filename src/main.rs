use clap::Parser;
use profile_json_refs::perf::timer::{PerfBucket, emit_buckets_stderr};
use profile_json_refs::{cli::CliArgs, config::ProfileConfig, run};
use std::time::Instant;

fn main() {
    let args = CliArgs::parse();

    match ProfileConfig::from_cli(args).and_then(run) {
        Ok(report) => {
            for warning in &report.warnings {
                eprintln!("WARNING {} {}", warning.code, warning.message);
            }

            let stdout_start = Instant::now();
            if !report.summary_quiet() {
                print!("{}", report.to_stdout_summary());
            }
            let stdout_duration = stdout_start.elapsed();

            if !report.perf_buckets.is_empty() {
                let mut buckets = report.perf_buckets.clone();
                buckets.push(PerfBucket {
                    name: "stdout.summary",
                    duration: stdout_duration,
                });
                emit_buckets_stderr(&buckets);
            }
        }
        Err(err) => {
            eprintln!("ERROR {err}");
            std::process::exit(1);
        }
    }
}
