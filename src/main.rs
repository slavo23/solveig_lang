use clap::Parser;
use solveig_driver::{Compilation, CompileOptions};
use solveig_report::emit_diagnostics;
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
    let mut compilation = Compilation::new(&cli.project_root);

    let run_result = compilation.compile_and_run_project_with_options_and_args(
        &cli.entry,
        CompileOptions {
            dump_bytecode: cli.dump_bytecode,
            dump_evidence: cli.dump_evidence,
            verify_core: cli.verify_core,
        },
        &cli.program_args,
    );

    if !compilation.diagnostics().is_empty()
        && let Err(report_err) =
            emit_diagnostics(compilation.diagnostics(), compilation.source_map())
    {
        eprintln!("failed to print diagnostics: {report_err}");
    }

    if let Err(err) = run_result {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
