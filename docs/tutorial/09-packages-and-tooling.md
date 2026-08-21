# 9. Packages, CLI, and editor

A Solveig package is a directory with `solveig.toml` and a `src/` tree. The
package name is the root of its module namespace.

## Project layout

```text
weather/
├── solveig.toml
└── src/
    ├── main.sv
    └── report/
        └── format.sv
```

```toml
[package]
name = "weather"
version = "0.1.0"

[language]
entry = "main"
```

The files load as `weather.main` and `weather.report.format`. From `main.sv`, a
same-package import may be relative:

```solveig
import report.format(renderReport)
```

The fully qualified `weather.report.format` also works.

## Local library dependencies

Suppose `app/` and `formatting/` are sibling packages. Add a path dependency to
`app/solveig.toml`:

```toml
[package]
name = "app"
version = "0.1.0"

[dependencies]
formatting = { path = "../formatting" }

[language]
entry = "main"
```

If the dependency declares `[package].name = "formatting"`, import it with:

```solveig
import formatting.text(renderTitle)
```

The dependency table key is not an alias; the dependency's declared package
name determines its module prefix. Package names in one dependency graph must
be unique. Dependency cycles are rejected.

The current package model is local-first. Dependencies are filesystem paths;
there is no registry, version solver, lockfile, Git dependency, or
publish/install command. Versions are metadata and do not select a dependency.

## Prelude and standard library

The compiler supplies two library packages:

- `prelude` is implicit and brings its universal vocabulary into ordinary
  modules;
- `std` is available without a manifest dependency but its modules must be
  imported explicitly.

This means adding a function to `std.list` does not silently place it in every
program's namespace. If you edit installed library source, remember that the
binary also contains an embedded fallback. Set `SOLVEIG_LIB` to the `lib/`
directory you intend to use when you want selection to be unambiguous:

```sh
SOLVEIG_LIB=/absolute/path/to/lib solveig --project-root path/to/app
```

That directory must contain both `prelude/solveig.toml` and
`std/solveig.toml`.

## CLI use

Run a project:

```sh
solveig --project-root path/to/project
```

The manifest chooses the source module. `--entry name` chooses a top-level
definition in that module and defaults to `main`:

```sh
solveig --project-root path/to/project --entry demo
```

Pass program arguments after `--`:

```sh
solveig --project-root path/to/project -- input.txt --verbose
```

Read them with `std.env.getArgs`. The CLI also accepts `--dump-bytecode`,
`--dump-evidence`, and `--verify-core`; these are diagnostic compiler options,
not requirements for normal execution.

The current CLI runs packages and starts the language server with `solveig
lsp`. It does not provide a REPL, formatter command, package manager, or test
runner command.

## Scratch files

The editor can analyze a bare `.sv` file without a manifest by treating its
directory as a local scratch project. For an application, create a manifest:
package roots make module names, dependencies, and CLI behavior explicit.

## VS Code extension

Build the extension from the repository:

```sh
cd solveig_vscode
npm install
npx vsce package --out solveig.vsix
code --install-extension solveig.vsix --force
```

The extension starts `solveig lsp`. If `solveig` is not on the environment
inherited by VS Code, set `solveig.server.path` to an absolute executable path.

Current editor capabilities include:

- parser, resolver, kind, type, and exhaustiveness diagnostics;
- hover types and documentation;
- completion and signature help;
- go to definition, find references, document symbols, and workspace symbols;
- semantic highlighting and optional inlay hints;
- rename and code actions when the affected source is locally owned.

Dependency and compiler-library source is readable and navigable. IDE-generated
mutations are restricted to local source, so a rename that would edit a loaded
dependency is refused as one operation. This does not make the dependency
read-only on disk: open an ordinary path dependency as its own project when you
intend to refactor it, or edit library source manually.

## Diagnose a problem

When compilation fails, start with the first diagnostic and its source span.
Later errors may be consequences of the same unresolved name or type mismatch.
Useful checks are:

1. Does `--project-root` point at the directory containing `solveig.toml`?
2. Does `[language].entry` map to an existing `.sv` path below `src/`?
3. Is the import relative to the current package or qualified by the dependency
   package's declared name?
4. If two modules export one name, can the use be qualified?
5. Does every `case` and equation group cover all constructors?
6. If `SOLVEIG_LIB` is set, does it point at the directory containing both
   shipped packages?

The [package reference](../packages.md), [diagnostics reference](../diagnostics.md),
and [language-server guide](../lsp.md) provide the detailed lookup tables.

## Where to go next

You now have the main language model:

- pure functions and algebraic data;
- explicit, typed effects;
- structured resource ownership;
- bounded concurrent work;
- TCP and HTTP interaction;
- generic parsing and direct JSON decoding;
- local packages and editor support.

Build a small package whose domain you understand, keep pure transformation
separate from IO boundaries, and add concurrency only around independent waits
that have a clear owner and a meaningful bound. Use the API reference when you
need exact signatures; the source doc comments are also visible on hover.

Return to the [tutorial index](README.md) or open the
[standard-library reference](../standard-library.md).
