# Solveig

Solveig is a small typed functional programming language and compiler written
in Rust. It has a real front-to-back pipeline: parser, resolver, kind checker,
bidirectional type checker with typeclass evidence, lowering, closure
conversion, bytecode generation, and execution on a custom VM.

The language is still experimental, but it is now practical enough to run
nontrivial examples, interactive terminal programs, and simple text-file IO.

## Install

Solveig has two pieces:

- the `solveig` command, which builds, checks, and runs Solveig projects
- the VS Code extension, which adds editor features for `.sv` files

Install the `solveig` command first. The VS Code extension starts that command
automatically when you open Solveig files.

### Prerequisites

You need a Rust toolchain with Cargo. If `cargo --version` works in your
terminal, you are ready to install Solveig.

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
cargo install --path .
```

Cargo installs the command as `solveig`, usually into `~/.cargo/bin` on macOS
and Linux or `%USERPROFILE%\.cargo\bin` on Windows.

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

### Uninstall the Solveig Command

If you installed Solveig with Cargo, uninstall it with:

```bash
cargo uninstall solveig
```

If you only built the repository with `cargo build`, nothing was installed
system-wide. If you manually copied a `solveig` binary somewhere, remove that
copied file from the directory where you put it.

## Quick Start

Build the compiler:

```bash
cargo build --workspace
```

Run the smoke example suite:

```bash
cargo run -- --project-root examples/smoke_suite
```

Run the interactive quiz example:

```bash
cargo run -- --project-root examples/quiz_game
```

Run the Scry mini rule engine:

```bash
cargo run -- --project-root examples/scry < examples/scry/sample-input.txt
```

Run all tests:

```bash
cargo test --workspace
```

Smoke-test the language server diagnostics path:

```bash
cargo build
python3 scripts/lsp_smoke.py tests/golden/diag/type_mismatch
```

Run clippy:

```bash
cargo clippy --workspace --all-targets
```

## Command Line

The CLI runs a Solveig project by pointing at a project root:

```bash
cargo run -- --project-root <path>
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
cargo run -- --project-root examples/smoke_suite
cargo run -- --project-root examples/smoke_suite --dump-bytecode
cargo run -- --project-root examples/quiz_game
cargo run -- --project-root my-project -- first second third
```

Inside Solveig, program args are available through:

```solveig
getArgs : IO [String]
```

Start the language server:

```bash
cargo run -- lsp
```

Or, after building:

```bash
target/debug/solveig lsp
```

This command is meant to be launched by an editor or test client, so running it
directly in a terminal waits for editor messages rather than showing a prompt.

## Language Server

Solveig ships editor support through `solveig lsp`. Editors normally start this
command for you, so running it directly in a terminal waits for editor messages
rather than showing a prompt.

Current editor features include errors and warnings, hover information,
go-to-definition, find references, document outline, workspace symbol search,
semantic highlighting, quick fixes, completion, inlay hints, highlights,
folding, selection expansion, signature help, rename, and clickable module
imports.

For capability details and the robustness testing plan, see
[`docs/lsp.md`](docs/lsp.md).

Manual smoke test:

```bash
cargo build
python3 scripts/lsp_smoke.py tests/golden/diag/type_mismatch
```

Expected output includes a `TC0005` type mismatch diagnostic with its editor
range.

## VS Code Extension

The VS Code extension lives in `solveig_vscode/`. It starts the `solveig`
command automatically for `.sv` files and provides errors, hover, completion,
rename, go to definition, find references, document outline, folding, signature
help, and module links.

Install the `solveig` command first and check that VS Code will be able to find
it:

```bash
solveig --help
```

After the extension is published, install it by extension id:

```bash
code --install-extension solveig-lang.solveig
```

Until then, install from a packaged `.vsix` file:

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

Uninstall the extension:

```bash
code --uninstall-extension solveig-lang.solveig
```

Uninstall the `solveig` command separately if you no longer want it:

```bash
cargo uninstall solveig
```

See `solveig_vscode/README.md` for the full extension guide and contributor
workflow.

## Project Layout

A Solveig project is a directory with source files under `src/`.

```text
my-project/
`-- src/
    |-- main.sv
    `-- math/
        `-- stats.sv
```

The compiler looks for:

```text
<project-root>/src/main.sv
```

Module names come from file paths:

```text
src/main.sv          -> main
src/math/stats.sv    -> math.stats
```

The default entry is `main.main`, written in source as:

```solveig
main : Int
main = 42
```

Run it with:

```bash
cargo run -- --project-root my-project
```

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
- monadic `IO`
- indentation-sensitive `do` notation over the generic `Monad` class
- terminal input/output
- program args through `getArgs`
- project-relative UTF-8 text file IO
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
main : IO ()
main = do
    printStr "name> "
    name <- readLine
    printLine (appendString "hello, " name)
```

Text files:

```solveig
main : IO ()
main = do
    contents <- readFile "input.txt"
    writeFile "copy.txt" contents
```

## Prelude

Ordinary modules implicitly import:

- `prelude.builtins`
- `prelude.list`
- `prelude.typeclasses`

The low-level `prelude.prim` module exists for primitive operations, but it is
not imported implicitly. Most programs should use the public Prelude wrappers
and typeclasses instead.

See [docs/prelude.md](docs/prelude.md) for signatures and examples.

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
- File IO is whole-file UTF-8 text IO. There are no handles or binary file
  operations yet.
- `/` is currently `Fractional` and only has a `Float` instance. Use `div` and
  `mod` for Int division and modulo.
- `String` concatenation is `appendString`; `+` is numeric.
- Float literal patterns are rejected.
- Constructor operators are reserved for later work.
- The REPL is planned but not complete.
- The LSP currently runs over stdio and has no packaged editor extension yet.
  It publishes diagnostics and supports hover, go-to-definition, references,
  document symbols, workspace symbols, semantic tokens, quick-fix code actions,
  completion, inlay hints, highlights, folding, selection ranges, signature
  help, rename, and module document links.

## More Documentation

- [docs/tutorial.md](docs/tutorial.md)
- [docs/prelude.md](docs/prelude.md)
- [docs/diagnostics.md](docs/diagnostics.md)
