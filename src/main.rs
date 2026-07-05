use clap::Parser;
use profile_json_refs::perf::timer::{PerfBucket, append_bucket};
use profile_json_refs::{cli::CliArgs, config::ProfileConfig, run};
use std::time::Instant;

fn main() {
    let main_start = Instant::now();
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

            if report.perf_enabled {
                let bucket = PerfBucket {
                    name: "stdout.summary",
                    duration: stdout_duration,
                };
                if let Err(err) = append_bucket(report.perf_log_file.as_deref(), &bucket) {
                    eprintln!("ERROR failed to write perf log: {err}");
                    std::process::exit(1);
                }
                let bucket = PerfBucket {
                    name: "main.total_wall",
                    duration: main_start.elapsed(),
                };
                if let Err(err) = append_bucket(report.perf_log_file.as_deref(), &bucket) {
                    eprintln!("ERROR failed to write perf log: {err}");
                    std::process::exit(1);
                }
            }
        }
        Err(err) => {
            eprintln!("ERROR {err}");
            std::process::exit(1);
        }
    }
}
