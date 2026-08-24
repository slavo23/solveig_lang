//! Filesystem and terminal orchestration for the `solveig fmt` command.
//! The formatter remains a pure CST transformation; this CLI boundary selects,
//! validates, displays, and applies its output.

use solveig_base::Session;
use solveig_driver::Compilation;
use solveig_ide::CompilationIdeExt;
use solveig_package::{PackageKind, is_package_root};
use solveig_report::emit_diagnostics;
use solveig_syntax::lowering::parse_source;
use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

pub(super) fn run_stdin(check: bool, width: usize) -> bool {
    if width == 0 {
        eprintln!("formatter line width must be greater than zero");
        return false;
    }
    let mut source = String::new();
    if let Err(error) = io::stdin().read_to_string(&mut source) {
        eprintln!("failed to read Solveig source from standard input: {error}");
        return false;
    }
    let path = Path::new("<stdin>");
    let Some(formatted) = format_input(path, &source, width) else {
        return false;
    };

    if check {
        if source == formatted {
            return true;
        }
        if let Err(error) = write_diff(&mut io::stdout().lock(), path, &source, &formatted) {
            eprintln!("failed to write formatter diff: {error}");
        }
        return false;
    }

    if let Err(error) = io::stdout().lock().write_all(formatted.as_bytes()) {
        eprintln!("failed to write formatted Solveig source to standard output: {error}");
        return false;
    }
    true
}

pub(super) fn run_files(project_root: &Path, paths: &[PathBuf], check: bool, width: usize) -> bool {
    if width == 0 {
        eprintln!("formatter line width must be greater than zero");
        return false;
    }
    let package_root = local_package_root(project_root);
    let files = if paths.is_empty() {
        let mut compilation = Compilation::new(package_root.as_deref().unwrap_or(project_root));
        if !load_project(&mut compilation) {
            return false;
        }
        let Some(files) = local_source_files(&compilation) else {
            return false;
        };
        if !sources_are_editable(&mut compilation, &files) {
            return false;
        }
        files
    } else {
        let Some(files) = selected_source_files(paths) else {
            return false;
        };
        if let Some(package_root) = package_root {
            let mut compilation = Compilation::new(package_root);
            if !load_project(&mut compilation) || !sources_are_editable(&mut compilation, &files) {
                return false;
            }
        } else {
            for path in &files {
                let mut compilation = Compilation::for_source_file(path);
                if !load_project(&mut compilation)
                    || !sources_are_editable(&mut compilation, std::slice::from_ref(path))
                {
                    return false;
                }
            }
        }
        files
    };
    if files.is_empty() {
        eprintln!("no Solveig source files were selected");
        return false;
    }

    let mut changes = Vec::new();
    for path in files {
        let source = match fs::read_to_string(&path) {
            Ok(source) => source,
            Err(error) => {
                eprintln!("failed to read `{}`: {error}", path.display());
                return false;
            }
        };
        let Some(formatted) = format_input(&path, &source, width) else {
            return false;
        };
        if source != formatted {
            changes.push((path, source, formatted));
        }
    }

    if check {
        let mut stdout = io::stdout().lock();
        for (path, source, formatted) in &changes {
            if let Err(error) = write_diff(&mut stdout, path, source, formatted) {
                eprintln!("failed to write formatter diff: {error}");
                return false;
            }
        }
        return changes.is_empty();
    }

    for (path, _source, formatted) in changes {
        if let Err(error) = fs::write(&path, formatted) {
            eprintln!("failed to write `{}`: {error}", path.display());
            return false;
        }
    }
    true
}

fn local_package_root(project_root: &Path) -> Option<PathBuf> {
    project_root
        .canonicalize()
        .ok()?
        .ancestors()
        .find(|candidate| is_package_root(candidate))
        .map(Path::to_path_buf)
}

fn load_project(compilation: &mut Compilation) -> bool {
    if let Err(error) = compilation.ensure_checked(false) {
        eprintln!("failed to load Solveig project: {error}");
        return false;
    }
    true
}

fn sources_are_editable(compilation: &mut Compilation, files: &[PathBuf]) -> bool {
    for path in files {
        if let Err(error) = compilation.ensure_source_editable(path) {
            eprintln!("{error}");
            return false;
        }
    }
    true
}

fn local_source_files(compilation: &Compilation) -> Option<Vec<PathBuf>> {
    let Some(snapshot) = compilation.snapshot() else {
        eprintln!("failed to load the Solveig package graph");
        return None;
    };
    Some(
        snapshot
            .project
            .modules
            .values()
            .filter(|module| {
                snapshot.project.package_kind(module.package) == Some(PackageKind::Local)
            })
            .map(|module| module.path.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    )
}

fn selected_source_files(paths: &[PathBuf]) -> Option<Vec<PathBuf>> {
    let mut files = BTreeSet::new();
    for path in paths {
        if !collect_source_files(path, &mut files) {
            return None;
        }
    }
    Some(files.into_iter().collect())
}

fn collect_source_files(path: &Path, files: &mut BTreeSet<PathBuf>) -> bool {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            eprintln!("failed to inspect `{}`: {error}", path.display());
            return false;
        }
    };
    if metadata.is_file() {
        if path.extension().is_some_and(|extension| extension == "sv") {
            return match path.canonicalize() {
                Ok(path) => {
                    files.insert(path);
                    true
                }
                Err(error) => {
                    eprintln!("failed to resolve `{}`: {error}", path.display());
                    false
                }
            };
        }
        eprintln!("`{}` is not a Solveig source file", path.display());
        return false;
    }
    if !metadata.is_dir() {
        eprintln!("`{}` is neither a file nor a directory", path.display());
        return false;
    }

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("failed to read directory `{}`: {error}", path.display());
            return false;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                eprintln!("failed to read an entry in `{}`: {error}", path.display());
                return false;
            }
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                eprintln!("failed to inspect `{}`: {error}", entry.path().display());
                return false;
            }
        };
        if file_type.is_dir() {
            if !collect_source_files(&entry.path(), files) {
                return false;
            }
        } else if file_type.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "sv")
        {
            match entry.path().canonicalize() {
                Ok(path) => {
                    files.insert(path);
                }
                Err(error) => {
                    eprintln!("failed to resolve `{}`: {error}", entry.path().display());
                    return false;
                }
            }
        }
    }
    true
}

fn format_input(path: &Path, source: &str, width: usize) -> Option<String> {
    let session = Session::new();
    let file = session.add_file(path, source);
    let parsed = parse_source(&session, file, source);
    if session.diagnostics().is_empty() {
        return Some(solveig_fmt::format(&parsed.cst, source, width));
    }

    let diagnostics = session.diagnostics().sorted_by_file_and_span();
    let source_map = session.source_map();
    if let Err(error) = emit_diagnostics(&diagnostics, &source_map) {
        eprintln!("failed to print formatter diagnostics: {error}");
    }
    None
}

fn write_diff(
    output: &mut dyn Write,
    path: &Path,
    source: &str,
    formatted: &str,
) -> io::Result<()> {
    writeln!(output, "--- {}", path.display())?;
    writeln!(output, "+++ {} (formatted)", path.display())?;
    writeln!(
        output,
        "@@ -1,{} +1,{} @@",
        source.lines().count(),
        formatted.lines().count()
    )?;
    for line in source.lines() {
        writeln!(output, "-{line}")?;
    }
    for line in formatted.lines() {
        writeln!(output, "+{line}")?;
    }
    Ok(())
}
