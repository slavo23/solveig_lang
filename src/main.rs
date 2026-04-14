mod compile;
mod project;
mod resolver;

use clap::Parser;
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
}

fn main() {
    let cli = Cli::parse();

    if let Err(err) =
        compile::compile_and_run_project(&cli.project_root, &cli.entry, cli.dump_bytecode)
    {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
