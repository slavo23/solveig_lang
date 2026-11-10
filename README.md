# Solveig

Solveig is an experimental, statically typed functional language for writing
clear programs with practical effects. It combines algebraic data types,
pattern matching, typeclasses, typed `IO`, structured concurrency, and a
standard library for files, processes, networking, HTTP, time, and parsing.

Solveig programs compile to bytecode and run on the Solveig virtual machine.
The language is under active development, and this repository is its
authoritative distribution.

## A small taste

```solveig
data Greeting = Formal String | Casual String

renderGreeting : Greeting -> String
renderGreeting (Formal name) = "Good day, " <> name <> "."
renderGreeting (Casual name) = "Hello, " <> name <> "!"

main : Task ()
main = printLine $ renderGreeting $ Casual "Solveig"
```

This example is intentionally independent of the applications developed in the
tutorial.

## Install

You need a current Rust toolchain with Cargo:

```sh
git clone https://github.com/slavo23/solveig_lang.git
cd solveig_lang
cargo install --path . --force
```

The installed executable contains the Prelude and standard library. Verify it
with:

```sh
solveig --help
```

Run a project by passing the directory containing its `solveig.toml`:

```sh
solveig --project-root path/to/project
```

The [first tutorial chapter](docs/tutorial/01-getting-started.md) explains the
repository build workflow, project layout, compiler diagnostics, and formatting.

## Learn and reference

- [Tutorial](docs/tutorial/README.md) — a guided course built around two real
  applications.
- [Documentation map](docs/README.md) — practical guides, API references, and
  implementation material.
- [Prelude reference](docs/prelude.md) — types, operators, typeclasses, and
  universal functions.
- [Standard-library reference](docs/standard-library.md) — available modules
  and their responsibilities.
- [Packages and modules](docs/packages.md) — manifests, imports, visibility,
  re-exports, and local dependencies.
