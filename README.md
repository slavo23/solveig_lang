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

Run all tests:

```bash
cargo test --workspace
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
- typed holes, used by the checker and future LSP work

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

The VM remains the reference runtime. A Cranelift backend is planned later.

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
- Constructor operators and backtick infix are reserved for later work.
- The Cranelift backend, REPL, and LSP server are planned but not complete.

## More Documentation

- [docs/tutorial.md](docs/tutorial.md)
- [docs/prelude.md](docs/prelude.md)
- [docs/diagnostics.md](docs/diagnostics.md)
