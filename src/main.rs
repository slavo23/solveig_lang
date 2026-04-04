mod resolver;

use std::fs;
use std::path::{Path, PathBuf};

use base::utils::diagnostics::emit_type_errors;
use chumsky::{input::Input as _, Parser as _};
use clap::Parser;
use resolver::Resolver;
use surface::parser::{lexer, program_parser};
use syntax::inference::type_infer::{check_and_elab_module, CheckedModule};

pub mod compile;
use compile::{interpret::Machine, lowering::lower_module};

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(value_name = "FILE")]
    path: PathBuf,
    #[arg(short, long, default_value = "main")]
    entry: String,
}

fn main() {
    let cli = Cli::parse();

    if let Err(err) = compile_and_run_file(&cli.path, &cli.entry) {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn compile_and_run_file(path: &Path, entry_name: &str) -> Result<(), String> {
    let src = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {}", path.display(), err))?;

    let checked = compile_source(&src, &path.display().to_string())?;
    let ir = lower_module(&checked);
    let result = Machine::exec(&ir, entry_name);

    println!("{}", result.display(&ir));
    Ok(())
}

fn compile_source(src: &str, file_id: &str) -> Result<CheckedModule, String> {
    let tokens = lexer()
        .parse(src)
        .into_result()
        .map_err(|errs| format!("lex error:\n{errs:#?}"))?;

    let input = tokens[..].split_spanned((0..tokens.len()).into());
    let parser = program_parser();
    let ast_result = parser.parse(input).into_result();
    let ast = ast_result.map_err(|errs| format!("parse error:\n{errs:#?}"))?;

    let resolver = Resolver::new();
    let resolved = resolver
        .resolve_surface_program(ast)
        .map_err(|errs| format!("resolve error:\n{errs:#?}"))?;

    match check_and_elab_module(&resolved, file_id) {
        Ok(checked) => Ok(checked),
        Err(errors) => {
            emit_type_errors(&errors, src)
                .map_err(|err| format!("failed to print type errors: {err}"))?;
            Err("type error".to_string())
        }
    }
}
