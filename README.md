# Solveig

Solveig is a small typed functional programming language and compiler written
in Rust. It has a real front-to-back pipeline: parser, resolver, kind checker,
bidirectional type checker with typeclass evidence, lowering, closure
conversion, bytecode generation, and execution on a custom VM.

The language is still experimental, but it is now practical enough to run
nontrivial examples, interactive terminal programs, and simple text-file IO.

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

The language server speaks LSP over stdio. It is meant to be launched by an
editor or test client, so running it directly in a terminal waits for protocol
messages rather than showing a prompt.

## Language Server

Solveig ships a stdio LSP server through `solveig lsp`. The current server
implements the lifecycle, full document sync, UTF-8/UTF-16 position negotiation,
debounced project checks, `textDocument/publishDiagnostics` using the same
diagnostic codes as the CLI, and Markdown `textDocument/hover` responses for
typed expressions, definitions, constructors, class methods, and module imports.
It also supports `textDocument/definition`, project-wide
`textDocument/references`, hierarchical `textDocument/documentSymbol`, and
project-wide `workspace/symbol` queries for Solveig declarations. Semantic
tokens are available through `textDocument/semanticTokens/full` with whole-file
delta responses. Quick fixes are available through `textDocument/codeAction`
for typed-hole fills, missing instance method stubs, missing imports, and
adding inferred type signatures. Completion is available through
`textDocument/completion` for locals, current-module declarations, imported
exports, constructors, class methods, keywords, and qualified module paths after
`.`. Inlay hints are available through `textDocument/inlayHint` for inferred
types on unannotated `let` bindings, unannotated lambda parameters, and
definitions without explicit signatures.

Manual smoke test:

```bash
cargo build
python3 scripts/lsp_smoke.py tests/golden/diag/type_mismatch
```

Expected output includes a negotiated `positionEncoding` and a `TC0005` type
mismatch diagnostic with its LSP range. Editor-extension wiring is a planned
Phase H follow-up.

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
  document symbols, workspace symbols, semantic tokens, and quick-fix code
  actions.

## More Documentation

- [docs/tutorial.md](docs/tutorial.md)
- [docs/prelude.md](docs/prelude.md)
- [docs/diagnostics.md](docs/diagnostics.md)
