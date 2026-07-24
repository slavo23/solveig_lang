use clap::Parser;
use solveig_driver::{CompileOptions, compile_and_run_project_with_options_and_args};
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
    #[arg(last = true)]
    program_args: Vec<String>,
}

fn main() {
    let cli = Cli::parse();

    if let Err(err) = compile_and_run_project_with_options_and_args(
        &cli.project_root,
        &cli.entry,
        CompileOptions {
            dump_bytecode: cli.dump_bytecode,
            dump_evidence: cli.dump_evidence,
            verify_core: cli.verify_core,
        },
        &cli.program_args,
    ) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
