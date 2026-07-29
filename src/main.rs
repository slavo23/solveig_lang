use clap::{Parser, Subcommand};
use solveig_driver::{Compilation, CompileOptions};
use solveig_report::emit_diagnostics;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    run: RunArgs,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Start the Solveig language server over stdio.
    Lsp,
}

#[derive(Debug, Parser)]
struct RunArgs {
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
    if let Some(Command::Lsp) = cli.command {
        if let Err(err) = solveig_lsp::run_stdio_blocking() {
            eprintln!("failed to start Solveig language server: {err}");
            std::process::exit(1);
        }
        return;
    }

    let mut compilation = Compilation::new(&cli.run.project_root);

    let run_result = compilation.compile_and_run_project_with_options_and_args(
        &cli.run.entry,
        CompileOptions {
            dump_bytecode: cli.run.dump_bytecode,
            dump_evidence: cli.run.dump_evidence,
            verify_core: cli.run.verify_core,
        },
        &cli.run.program_args,
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
