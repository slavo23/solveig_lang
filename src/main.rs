use clap::Parser;
use solveig_driver::{CompileOptions, compile_and_run_project_with_options};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(long, default_value = ".")]
    project_root: PathBuf,
    #[arg(short, long, default_value = "main")]
    entry: String,
    #[arg(long, default_value = "false")]
    dump_bytecode: bool,
    #[arg(long, default_value = "false")]
    dump_evidence: bool,
    #[arg(long, default_value = "false")]
    verify_core: bool,
}

fn main() {
    let cli = Cli::parse();

    if let Err(err) = compile_and_run_project_with_options(
        &cli.project_root,
        &cli.entry,
        CompileOptions {
            dump_bytecode: cli.dump_bytecode,
            dump_evidence: cli.dump_evidence,
            verify_core: cli.verify_core,
        },
    ) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
