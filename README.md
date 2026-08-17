# Solveig

Solveig is a small typed functional programming language and compiler written
in Rust. It has a real front-to-back pipeline: parser, resolver, kind checker,
bidirectional type checker with typeclass evidence, lowering, closure
conversion, bytecode generation, and execution on a custom VM.

The language is still experimental, but it is now practical enough to run
nontrivial examples, interactive terminal programs, and simple text-file IO.

## Install

Solveig has two installable pieces:

- the `solveig` command, which builds, checks, and runs Solveig projects
- the VS Code extension, which adds editor features for `.sv` files

Install the `solveig` command first. The VS Code extension starts that command
automatically when you open Solveig files.

### Prerequisites

The current installer builds Solveig from source, so you need a Rust toolchain
with Cargo. Cargo is only used to install, update, or remove the binary. After
installation, you run Solveig programs with the `solveig` command.

If `cargo --version` works in your terminal, you are ready to install Solveig.

For the VS Code extension, you also need Visual Studio Code. The terminal
commands below use VS Code's `code` command; if `code --version` does not work,
you can install extensions through the VS Code Extensions view instead.

### Install the Solveig Command

Install directly from the repository:

```bash
cargo install --git https://github.com/slavo23/lang_experiments.git
```

Or install from a local checkout:

```bash
git clone https://github.com/slavo23/lang_experiments.git
cd lang_experiments
just install
```

The all-in-one command requires [`just`](https://github.com/casey/just) in
addition to the toolchain prerequisites above.

`just install` installs the command and VS Code extension, then copies the
shipped compiler libraries beside the executable. A direct `cargo install`
still runs through the embedded fallback, but installing the files is preferred
because editor navigation can open Prelude definitions as ordinary source.

Cargo installs the executable as `solveig`. The usual install locations are:

- macOS and Linux: `~/.cargo/bin/solveig`
- Windows: `%USERPROFILE%\.cargo\bin\solveig.exe`

The library directory sits beside that executable:

```text
bin/
|-- solveig
`-- lib/
    |-- prelude/
    `-- std/
```

Packagers should preserve that layout. `SOLVEIG_LIB` may instead point at a
different `lib` directory containing both package manifests. Library discovery
uses the override first, then `lib` beside the executable, and finally the
copy embedded in the binary. The selected path and source are written to the
compiler's tracing output.

Check the installation:

```bash
solveig --help
```

If your shell cannot find `solveig`, make sure Cargo's bin directory is on your
PATH. On macOS or Linux, this is usually:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

Add that line to your shell profile if you want it to apply to future terminal
sessions.

On Windows, add `%USERPROFILE%\.cargo\bin` to your user `Path` environment
variable, then restart your terminal and VS Code.

To update a Cargo-installed Solveig binary later, run the same install command
again with `--force`:

```bash
cargo install --git https://github.com/slavo23/lang_experiments.git --force
```

From a local checkout:

```bash
git pull
just install
```

### Uninstall the Solveig Command

If you installed Solveig with Cargo, uninstall it with:

```bash
cargo uninstall solveig
```

This removes the installed `solveig` executable from Cargo's bin directory. It
does not remove your Solveig projects or this repository checkout.

If you manually copied a `solveig` binary somewhere, delete that copied file
from the directory where you placed it. If you only built the repository for
development, nothing was installed system-wide.

## Quick Start

After installing, check that the command is available:

```bash
solveig --help
```

Run the smoke example suite:

```bash
solveig --project-root examples/smoke_suite
```

Run the interactive quiz example:

```bash
solveig --project-root examples/quiz_game
```

Run the Scry mini rule engine:

```bash
solveig --project-root examples/scry < examples/scry/sample-input.txt
```

Run the concurrent internet HTTP downloader:

```bash
solveig --project-root examples/concurrent_downloader
```

Pass `-- --local` for its deterministic in-program server mode.

## Command Line

The CLI runs a Solveig project by pointing at a project root:

```bash
solveig --project-root <path>
```

Useful options:

```text
--project-root <path>   Path to the Solveig project root. Defaults to `.`
-e, --entry <name>      Entry function inside the `main` module. Defaults to `main`
--dump-bytecode         Write lowered bytecode to `bytecode.txt`
--dump-evidence         Print solved typeclass evidence
--verify-core           Run the core linter during compilation
--                      Remaining arguments are passed to Solveig `getArgs`
```

Examples:

```bash
solveig --project-root examples/smoke_suite
solveig --project-root examples/smoke_suite --dump-bytecode
solveig --project-root examples/quiz_game
solveig --project-root examples/concurrent_downloader
solveig --project-root my-project -- first second third
```

Inside Solveig, program args are available through:

```solveig
getArgs : forall e. IO e [String]
```

Start the language server:

```bash
solveig lsp
```

This command is meant to be launched by an editor or test client, so running it
directly in a terminal waits for editor messages rather than showing a prompt.

## Language Server

Solveig ships editor support through `solveig lsp`. Editors normally start this
command for you, so running it directly in a terminal waits for editor messages
rather than showing a prompt.

Current editor features include errors and warnings, hover information,
go-to-definition, find references, document outline, workspace symbol search,
semantic highlighting, quick fixes, completion, optional inline type hints,
highlights, folding, selection expansion, signature help, rename, and clickable
module imports.

For capability details and the robustness testing plan, see
[`docs/lsp.md`](docs/lsp.md).

Manual smoke test:

```bash
SOLVEIG_BIN="$(command -v solveig)" python3 scripts/lsp_smoke.py tests/golden/diag/type_mismatch
```

Expected output includes a `TC0005` type mismatch diagnostic with its editor
range.

## VS Code Extension

The VS Code extension lives in `solveig_vscode/`. It starts the `solveig`
command automatically for `.sv` files and provides errors, hover, completion,
rename, go to definition, find references, document outline, folding, signature
help, and module links.

### Install the VS Code Extension

Install the `solveig` command first and check that VS Code will be able to find
it:

```bash
solveig --help
```

If the extension is available by id, install it from a terminal with:

```bash
code --install-extension solveig-lang.solveig
```

Otherwise, install from a packaged `.vsix` file:

```bash
code --install-extension /path/to/solveig-0.1.0.vsix
```

To build and install that package from this repository:

```bash
cd solveig_vscode
npm install
npm run package -- --out /tmp/solveig.vsix
code --install-extension /tmp/solveig.vsix --force
```

You can also install a `.vsix` from inside VS Code: open the Extensions view,
open the `...` menu, choose **Install from VSIX...**, and select the file.

If the `solveig` command is not on your PATH, set **Solveig: Server Path** in
VS Code settings to the executable path. A Cargo install usually puts the
binary here on macOS and Linux:

```json
{
  "solveig.server.path": "/Users/you/.cargo/bin/solveig"
}
```

On Windows, use the path to `solveig.exe`, for example:

```json
{
  "solveig.server.path": "C:\\Users\\you\\.cargo\\bin\\solveig.exe"
}
```

To use the extension, open a folder that contains Solveig source files and then
open any `.sv` file. The Solveig status item appears in the VS Code status bar.
If it shows an error, run **Solveig: Show Output** from the Command Palette.
The extension also offers a quick fix to add an inferred type signature for a
top-level definition when Solveig can infer one.

### Uninstall the VS Code Extension

The extension and the `solveig` command are installed separately. Remove the
extension if you no longer want editor support:

```bash
code --uninstall-extension solveig-lang.solveig
```

You can also uninstall it from VS Code: open the Extensions view, search for
Solveig, open the extension entry, and choose **Uninstall**.

Uninstall the `solveig` command separately if you no longer want the compiler
or language server command:

```bash
cargo uninstall solveig
```

If you set **Solveig: Server Path** to a custom binary, remove or update that
setting after uninstalling or moving the command.

See `solveig_vscode/README.md` for the full extension guide and contributor
workflow.

## Project Layout

A Solveig project has a strict `solveig.toml` manifest and source files under
`src/`.

```text
my-project/
|-- solveig.toml
`-- src/
    |-- main.sv
    `-- math/
        `-- stats.sv
```

The manifest names the package and its entry module:

```toml
[package]
name = "app"
version = "0.1.0"

[language]
entry = "main"
```

Manifest keys are checked strictly, so misspellings are errors. A path
dependency uses `name = { path = "../name" }` under `[dependencies]`.

Module names come from file paths:

```text
src/main.sv          -> app.main
src/math/stats.sv    -> app.math.stats
```

The package name is always the first module segment. Within the package,
`math.stats` is a convenient relative import; another package imports the same
module as `app.math.stats`.

The configured entry selects `src/main.sv`, loaded as `app.main` and written
in source as:

```solveig
main : Int
main = 42
```

Run it with:

```bash
solveig --project-root my-project
```

See [docs/packages.md](docs/packages.md) for the full manifest reference, path
dependencies, library search order, Prelude/std behavior, and IDE edit policy.

## Language Features

Solveig currently supports:

- algebraic data types
- pattern matching with exhaustiveness checking
- first-class functions and lambdas
- `let` expressions
- `if ... then ... else ...`
- modules, imports, aliases, and selective imports
- explicit `forall`
- higher-rank type annotations
- single-parameter typeclasses with superclasses
- typeclass evidence elaboration into runtime dictionaries
- default class methods
- orphan instance warnings
- user-defined symbolic operators
- Haskell-style fixity declarations
- operator sections and backticked infix names
- `Int`, `Float`, `Char`, `String`, `Bool`, and `()`
- list syntax: `[]`, `::`, and `[1, 2, 3]`
- `Eq`, `Ord`, `Num`, `Fractional`, `Functor`, `Applicative`, `Monad`, and `Show`
- value display through `Show`
- typed monadic `IO e a` with the `Task a = IO IOError a` application alias
- indentation-sensitive `do` notation over the generic `Monad` class
- terminal input/output
- program args through `getArgs`
- explicit buffered handles plus UTF-8 and arbitrary-byte file IO
- runtime-local environment and working-directory operations
- shell-free child-process capture and cleanup-safe program exit statuses
- nanosecond monotonic and Unix-epoch clocks with deterministic virtual time
- typed FIFO channels, interruption, scoped masking, lightweight timers, and
  structured `withThread`/`race`/`concurrently`/`timeout` combinators
- immutable byte strings, blocking-pool DNS, and readiness-driven TCP sockets
- structured diagnostics with stable codes
- typed holes with LSP diagnostics and quick-fix fill suggestions

## Small Examples

Functions:

```solveig
double : Int -> Int
double x = x + x

main : Int
main = double 21
```

Data and pattern matching:

```solveig
data Tree a = Leaf | Node (Tree a) a (Tree a)

size : forall a. Tree a -> Int
size tree =
    case tree of
        Leaf => 0
        | Node left value right => 1 + size left + size right

main : Int
main = size (Node Leaf 10 (Node Leaf 20 Leaf))
```

Lists:

```solveig
sum : [Int] -> Int
sum xs =
    case xs of
        [] => 0
        | x :: rest => x + sum rest

main : [Int]
main = map (fun x => x * x) [1, 2, 3, 4]
```

IO:

```solveig
main : Task ()
main = do
    printStr "name> "
    name <- readLine
    printLine (appendString "hello, " name)
```

Text files:

```solveig
main : Task ()
main = do
    contents <- readFile "input.txt"
    writeFile "copy.txt" contents
```

Deterministic pseudorandom values use explicit immutable state:

```solveig
import std.random(randomRange, mkStdGen)

case randomRange 1 6 (mkStdGen 42) of
    MkPair die nextGenerator => die
```

Use `newStdGen : Task StdGen` when a runtime-local clock seed is wanted, then
thread the generator returned by each draw. The generator is not suitable for
cryptographic use.

## Prelude

Ordinary modules implicitly import:

- `prelude.builtins`
- `prelude.list`
- `prelude.typeclasses`

The low-level `prelude.prim` module exists for primitive operations, but it is
not imported implicitly. Most programs should use the public Prelude and
explicit standard-library modules instead:

```solveig
import std.io(readFile, writeFile)
import std.list(take, zip)
import std.task(concurrently)
```

The shipped `std` package is available to every project but never automatic,
so adding a library API cannot silently introduce names into existing code.

See [docs/prelude.md](docs/prelude.md) for signatures and examples.
See [docs/standard-library.md](docs/standard-library.md) for the `std.*` module
map and practical import examples.
See [docs/errors.md](docs/errors.md) for typed errors, defects, cleanup, and
the interruption model.
See [docs/concurrency.md](docs/concurrency.md) for green threads, channels,
structured concurrency, timers, and sockets.

## Diagnostics

Diagnostics are phase-prefixed:

- `PAR0001` for parser errors
- `RES....` for name resolution, imports, fixity, and operators
- `KIND....` for kind errors
- `TC....` for type checking and typeclass errors or warnings

Example:

```solveig
main : String
main = "left" + "right"
```

This reports `TC0013` because `+` is the `Num` method and there is no
`Num String` instance.

See [docs/diagnostics.md](docs/diagnostics.md) for the full code index.

## Examples In This Repository

- `examples/smoke_suite` keeps the old all-in-one demonstration project. It
  imports and evaluates algorithms and typeclass stress cases.
- `examples/quiz_game` reads questions from `questions.txt`, prompts on stdin,
  scores answers, writes `last-score.txt`, and appends `score-history.txt`.
- `examples/scry` is a mini S-expression rule engine with facts, rules,
  recursive queries, file IO, and an interactive prompt.
- `examples/concurrent_downloader` resolves and fetches several plain-HTTP
  internet resources through a bounded-channel worker pool, writes four fixed
  ignored outputs, and retains a hermetic `--local` mode for tests.
- `examples/log_audit` is the Phase L capstone: three scoped workers read log
  files through bounded job/result channels, recover from a missing input,
  aggregate completion-order-independent counts, and write a fixed report.
- `tests/golden/run/` contains small executable regression cases.
- `tests/golden/diag/` contains diagnostic regression cases.

## Implementation Pipeline

The current VM pipeline is:

1. Parse source modules.
2. Resolve imports, names, constructors, classes, and operators.
3. Infer/check kinds.
4. Typecheck and elaborate expressions.
5. Solve typeclass constraints and insert evidence.
6. Run the core linter when requested and in tests.
7. Lower checked core to IR.
8. Desugar evidence into dictionaries.
9. Closure-convert.
10. Lower to bytecode.
11. Execute on the custom VM.

The VM remains the reference runtime. Native backends are currently out of
scope.

## Current Limitations

- `do` notation supports variable binds, `_` binds, local `let` statements, and
  expression statements. Pattern binds are reserved for a later failure-semantics
  decision.
- Captured subprocess output is strict UTF-8 and held in memory; streaming
  subprocess IO is not implemented yet.
- Open handles require explicit `hClose` or `withFile`; GC does not close host
  resources.
- `/` is currently `Fractional` and only has a `Float` instance. Use `div` and
  `mod` for Int division and modulo.
- `String` concatenation is `appendString`; `+` is numeric.
- Float literal patterns are rejected.
- Constructor operators are reserved for later work.
- The REPL is planned but not complete.
- The language server runs through the `solveig lsp` command. The VS Code
  extension starts it automatically for editor features.

## Contributor Checks

For compiler and extension development from this repository:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
```

Performance work uses the measured workloads and selective-run policy in
[docs/benchmarks.md](docs/benchmarks.md). The complete suite is not part of the
routine contributor check.

To smoke-test the installed language server command from macOS or Linux:

```bash
SOLVEIG_BIN="$(command -v solveig)" python3 scripts/lsp_smoke.py tests/golden/diag/type_mismatch
```

## More Documentation

- [docs/tutorial.md](docs/tutorial.md)
- [docs/packages.md](docs/packages.md)
- [docs/prelude.md](docs/prelude.md)
- [docs/standard-library.md](docs/standard-library.md)
- [docs/io.md](docs/io.md)
- [docs/errors.md](docs/errors.md)
- [docs/concurrency.md](docs/concurrency.md)
- [docs/benchmarks.md](docs/benchmarks.md)
- [docs/diagnostics.md](docs/diagnostics.md)
