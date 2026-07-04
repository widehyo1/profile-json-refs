use clap::Parser;
use profile_json_refs::{cli::CliArgs, config::ProfileConfig};

fn main() {
    let args = CliArgs::parse();

    match ProfileConfig::from_cli(args).and_then(|config| config.validate()) {
        Ok(()) => {}
        Err(err) => {
            eprintln!("ERROR {err}");
            std::process::exit(1);
        }
    }
}
