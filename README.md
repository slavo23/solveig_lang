# Solveig

Solveig is a small typed functional programming language and compiler project.

It is designed around a principled core:

- algebraic data types
- pattern matching
- first-class functions
- explicit polymorphism with `forall`
- a module system
- a compiler-owned prelude
- a simple custom bytecode VM

Solveig is not trying to be a production language yet. It is a language and compiler project meant to stay understandable, inspectable, and pleasant to evolve.

## What Solveig currently supports

At the language level, Solveig currently supports:

- modules and imports
- top-level signatures and function definitions
- algebraic data types
- pattern matching with `case ... of`
- `let ... in ...`
- `if ... then ... else ...`
- integer literals
- arithmetic and comparison operators
- list literals like `[1, 2, 3]`
- cons expressions and patterns with `::`
- explicit `forall` in types
- semicolon-separated top-level declarations

The compiler also ships with compiler-owned prelude modules. Non-prelude modules get `prelude.builtins` and `prelude.list` implicitly, so common list and builtin functionality is available without manual imports.

## High-level pipeline

Solveig currently goes through the following pipeline:

1. Parse surface modules
2. Resolve names and imports
3. Typecheck and elaborate modules
4. Lower to internal IR
5. Closure-convert to closure IR
6. Lower to bytecode
7. Execute on the custom VM

The project is structured so that modules are checked in dependency order before whole-program lowering and execution.

## Why this project exists

Solveig is a place to experiment with:

- typed functional language design
- explicit polymorphism and higher-rank ideas
- module-aware checking
- lowering and elaboration
- custom bytecode and VM design
- diagnostics and compiler architecture

The goal is not only to make the language more expressive, but also to keep the implementation clean enough that its behavior is easy to reason about.

## Building the compiler

Solveig is a Rust project.

Build it with:

```bash
cargo build --release
```

The default way to compile and run a Solveig project is to point the compiler at a project root:

```bash
cargo run -- --project-root example_project
```

This runs the project using the default entry configuration.

## Command line options

The compiler currently supports these CLI options:

```text
--project-root <path>   Path to the Solveig project root
-e, --entry <name>      Entry function inside the `main` module
--dump-bytecode         Dump lowered bytecode to `bytecode.txt`
```

Examples:

Compile and run a project:

```bash
cargo run -- --project-root example_project
```

Compile and run a project while dumping bytecode:

```bash
cargo run -- --project-root example_project --dump-bytecode
```

The CLI also has an `--entry` option for selecting a different entry inside the `main` module, but the standard project-level command above is the recommended way to run Solveig projects.

## Project layout

A Solveig project is a tree of `.sv` files under `src/`.

The compiler currently looks for the root entry source file at:

```text
<project-root>/src/main.sv
```

Module names are derived from file paths. For example:

```text
src/main.sv              -> main
src/math/list.sv         -> math.list
src/data/tree.sv         -> data.tree
```

A typical layout might look like this:

```text
my-project/
├── Cargo.toml
├── src/
│   ├── main.sv
│   ├── math/
│   │   └── list.sv
│   └── data/
│       └── tree.sv
```

## Imports

Solveig supports module imports, aliasing, and selective imports.

Examples:

```solveig
import math.list;
import math.list as list;
import math.list (sum, length);
```

In addition to explicit imports, ordinary modules implicitly receive the compiler-owned prelude modules.

## A quick syntax tour

### Top-level signatures and definitions

Top-level items are separated by semicolons.

```solveig
id : forall a. a -> a;
id x = x;

const : forall a b. a -> b -> a;
const x y = x;
```

### Data types

```solveig
data Tree a = Leaf | Node (Tree a) a (Tree a);
```

### Pattern matching

```solveig
size : forall a. Tree a -> Int;
size tree =
    case tree of
        Leaf => 0
      | Node left _ right => 1 + size left + size right;
```

### Lists

Lists can be written directly:

```solveig
xs : [Int];
xs = [1, 2, 3, 4];
```

And matched like this:

```solveig
sum : [Int] -> Int;
sum xs =
    case xs of
        [] => 0
      | x :: rest => x + sum rest;
```

### Let expressions

```solveig
doublePlusOne : Int -> Int;
doublePlusOne x =
    let y = x + x in
        y + 1;
```

### If expressions

```solveig
absInt : Int -> Int;
absInt n =
    if n < 0 then
        0 - n
    else
        n;
```

### Lambda expressions

```solveig
inc : Int -> Int;
inc =
    fun x => x + 1;
```

## A small example

```solveig
data Tree a = Leaf | Node (Tree a) a (Tree a);

insertInt : Int -> Tree Int -> Tree Int;
insertInt x tree =
    case tree of
        Leaf => Node Leaf x Leaf
      | Node left y right =>
            if x < y then
                Node (insertInt x left) y right
            else if x > y then
                Node left y (insertInt x right)
            else
                tree;

fromListInt : [Int] -> Tree Int;
fromListInt xs =
    case xs of
        [] => Leaf
      | x :: rest => insertInt x (fromListInt rest);
```

## Running code

If `src/main.sv` contains:

```solveig
main : Int;
main = 42;
```

then run:

```bash
cargo run -- --project-root example_project
```

The compiler lowers the checked program, runs it on the VM, and prints the result.

## Bytecode dumping

To inspect the lowered bytecode:

```bash
cargo run -- --project-root . --entry main --dump-bytecode
```

This writes a `bytecode.txt` file containing:

- globals
- constructors
- chunks
- instructions
- expanded match table information

This is useful when debugging lowering, pattern matching, closure conversion, or VM execution.

## Current examples and benchmarks

The repository already contains nontrivial examples and benchmark-style programs, including things like:

- binary tree code
- N-Queens
- Sudoku
- Brainfuck
- quicksort
- stress tests and VM benchmarks

These are useful both as language examples and as runtime/compiler tests.

## Current status

Solveig already has a meaningful end-to-end pipeline and can run substantial examples.

At the same time, it is still actively evolving. The current emphasis is on:

- keeping the design principled
- improving diagnostics and ergonomics
- growing the standard library
- refining the language surface
- adding features only when they fit the existing architecture cleanly

## Near-term direction

The most likely near-term areas of work are:

- cleaner project/compiler structure
- better diagnostics
- more examples and benchmarks
- more practical standard-library modules
- additional language features that fit the current core cleanly

## Notes

A few practical notes about the current state of the language:

- the prelude is compiler-owned
- modules are checked in dependency order
- import cycles are rejected
- the runtime is based on a custom bytecode VM
- the implementation is intentionally kept understandable rather than overly magical

## Summary

Solveig is a small, typed functional language project with a real compiler pipeline and custom VM.

It is a good fit for experimenting with:

- type systems
- elaboration
- lowering pipelines
- bytecode design
- runtime behavior
- compiler structure

while still having a language that is pleasant enough to write real examples in.
