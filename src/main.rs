use clap::{Parser, Subcommand};
use solveig_driver::{Compilation, CompileOptions};
use solveig_report::emit_diagnostics;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

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
        init_lsp_tracing();
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

    match run_result {
        Ok(status) if !status.is_success() => std::process::exit(status.host_code()),
        Ok(_) => {}
        Err(err) => {
            eprintln!("{err}");
            std::process::exit(1);
        }
    }
}

#[derive(Clone)]
struct LspTraceWriter {
    file: Arc<Mutex<Option<File>>>,
}

struct LspTraceWriteGuard {
    file: Arc<Mutex<Option<File>>>,
}

impl LspTraceWriter {
    fn new() -> (Self, Option<PathBuf>) {
        let path = lsp_log_path();
        let file = open_lsp_log_file(&path);
        let visible_path = file.as_ref().map(|_| path);

        (
            Self {
                file: Arc::new(Mutex::new(file)),
            },
            visible_path,
        )
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LspTraceWriter {
    type Writer = LspTraceWriteGuard;

    fn make_writer(&'a self) -> Self::Writer {
        LspTraceWriteGuard {
            file: Arc::clone(&self.file),
        }
    }
}

impl Write for LspTraceWriteGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let written = io::stderr().write(buf)?;
        let bytes = match buf.get(..written) {
            Some(bytes) => bytes,
            None => buf,
        };

        if !bytes.is_empty()
            && let Ok(mut guard) = self.file.lock()
        {
            let write_failed = match guard.as_mut() {
                Some(file) => file.write_all(bytes).is_err(),
                None => false,
            };
            if write_failed {
                *guard = None;
            }
        }

        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        let stderr_result = io::stderr().flush();

        if let Ok(mut guard) = self.file.lock() {
            let flush_failed = match guard.as_mut() {
                Some(file) => file.flush().is_err(),
                None => false,
            };
            if flush_failed {
                *guard = None;
            }
        }

        stderr_result
    }
}

fn init_lsp_tracing() {
    let (writer, log_path) = LspTraceWriter::new();
    if let Err(err) = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(writer)
        .try_init()
    {
        eprintln!("failed to initialize Solveig LSP tracing: {err}");
    }

    if let Some(log_path) = log_path {
        tracing::info!(
            path = %log_path.display(),
            "Solveig LSP tracing initialized"
        );
    }
}

fn lsp_log_path() -> PathBuf {
    std::env::var_os("SOLVEIG_LSP_LOG").map_or_else(
        || std::env::temp_dir().join("solveig-lsp.log"),
        PathBuf::from,
    )
}

fn open_lsp_log_file(path: &Path) -> Option<File> {
    if let Some(parent) = path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        eprintln!(
            "failed to create Solveig LSP log directory {}: {err}",
            parent.display()
        );
        return None;
    }

    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => Some(file),
        Err(err) => {
            eprintln!(
                "failed to open Solveig LSP log file {}: {err}",
                path.display()
            );
            None
        }
    }
}
