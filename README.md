# Solveig

Solveig is a small statically typed functional language with a compact, principled core. It currently focuses on algebraic data types, pattern matching, first-class functions, explicit polymorphism, higher-rank types, local bindings, and a simple module system.

## Overview

Solveig currently supports:

- algebraic data types
- pattern matching
- first-class functions
- explicit `forall`
- higher-rank polymorphism
- local `let ... in ...`
- integer literals, arithmetic, and comparisons
- list literals, list patterns, and list types
- a project-level module system
- compiler-provided prelude modules

A small example:

```hs
sumEvenBigList =
    let evenList = filter even bigList in
    foldl (fun acc x => acc + x) 0 evenList;

main = sumEvenBigList;
```

This matches the current repository examples and prelude style.

## Example syntax

### Data declarations

```hs
data Bool = True | False;
data Pair a b = MkPair a b;
data List a = Cons a (List a) | Nil;
```

### Typed functions

```hs
map : forall a b. (a -> b) -> List a -> List b;
map f xs =
    case xs of
        [] => []
        | y :: ys => (f y) :: map f ys;
```

### Conditionals and local bindings

```hs
filter : forall a. (a -> Bool) -> [a] -> [a];
filter f xs =
    case xs of
        [] => []
        | y :: ys => if (f y) then y :: filter f ys else filter f ys;
```

```hs
sumEvenBigList =
    let evenList = filter even bigList in
    foldl (fun acc x => acc + x) 0 evenList;
```

## Modules

Solveig projects are rooted at `src/`. Module names are derived directly from file paths under that directory.

Example layout:

```text
my_project/
  src/
    main.sv
    utils/
      list.sv
    data/
      tree/
        Tree.sv
```

Examples of the resulting module names:

- `src/main.sv` -> `main`
- `src/utils/list.sv` -> `utils.list`
- `src/data/tree/Tree.sv` -> `data.tree.Tree`

Imports refer to those module names. Typical forms include:

```hs
import utils.list;
import utils.list(bigList, foldl);
import collections.List as List;
import collections.List as List(map, foldr);
```

Modules are loaded as a project graph, checked for import cycles, and processed in dependency order before the whole program is lowered and executed.

## Prelude

Solveig currently ships with compiler-owned prelude modules rather than requiring users to place them in each project.

The important prelude modules are:

- `prelude.builtins`
- `prelude.list`

In practice this means:

- ordinary modules implicitly see `prelude.builtins` and `prelude.list`
- boolean syntax is canonical through `prelude.builtins`
- list syntax is canonical through `prelude.list`

The current prelude contains definitions such as:

- `Bool`, `True`, `False`
- `List`, `Nil`, `Cons`
- basic helpers such as `map`, `filter`, `foldr`, `foldl`, `append`, and `range`

Prelude modules are embedded into the compiler and loaded as virtual modules.

## Building projects

Solveig is currently built and run through Cargo.

Build the compiler:

```bash
cargo build
```

Build an optimized binary:

```bash
cargo build --release
```

## Running projects

The current driver expects a Solveig project root and looks for `src/main.sv` by default.

From the compiler repository root, run a project in the current directory:

```bash
cargo run --
```

Run a project at a different location:

```bash
cargo run -- --project-root path/to/project
```

Run a different entry definition from the `main` module:

```bash
cargo run -- --project-root path/to/project --entry someDefinition
```

With the release binary:

```bash
./target/release/<binary-name> --project-root path/to/project --entry main
```

Notes:

- the default project root is `.`
- the default entry definition is `main`
- the driver resolves the entry file as `src/main.sv`

## Minimal project

```text
my_project/
  src/
    main.sv
```

```hs
main = [1, 2, 3];
```

Then run it from `my_project/`:

```bash
cargo run --
```

## Current direction

Solveig is still under active development, but the current architecture is already centered on a stable set of ideas:

- explicit source-level polymorphism
- module-local checking with project-level dependency ordering
- canonical prelude-owned booleans and lists
- whole-program lowering after per-module checking

That keeps the surface language small while leaving room for the language and compiler to grow without changing its basic structure.
