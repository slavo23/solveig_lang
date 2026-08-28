# Solveig

Solveig is an experimental, statically typed functional language for writing
clear programs that also need practical effects: files, processes, TCP,
HTTP/HTTPS, timers, channels, and structured concurrency. Programs are compiled
to bytecode and run on Solveig's own virtual machine.

The language combines algebraic data types, pattern matching, typeclasses,
higher-rank type signatures, typed IO errors, and lightweight concurrent tasks.
The project is under active development, so this repository is currently the
authoritative distribution.

## Install from source

You need a current Rust toolchain with Cargo. Clone the repository, install the
binary, and copy the editable Prelude and standard-library sources beside it:

```sh
git clone https://github.com/slavo23/solveig_lang.git
cd solveig_lang
cargo install --path . --force

INSTALL_ROOT="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}"
mkdir -p "$INSTALL_ROOT/bin/lib/prelude" "$INSTALL_ROOT/bin/lib/std"
cp -R lib/prelude/. "$INSTALL_ROOT/bin/lib/prelude"
cp -R lib/std/. "$INSTALL_ROOT/bin/lib/std"
```

The binary contains a fallback copy of both libraries, so `cargo install --path
.` is enough to run programs. Copying `lib/` is recommended because editor
navigation can then open their real source files. `SOLVEIG_LIB` can point to a
different directory containing both `prelude/solveig.toml` and
`std/solveig.toml`.

Make sure Cargo's binary directory is on `PATH`, then verify the command:

```sh
solveig --help
```

To build without installing anything, use `cargo build --release` and run
`target/release/solveig`. The checkout's library sources are embedded in that
binary at build time.

## Your first project

Create this layout:

```text
hello/
├── solveig.toml
└── src/
    └── main.sv
```

`hello/solveig.toml`:

```toml
[package]
name = "hello"
version = "0.1.0"

[language]
entry = "main"
```

`hello/src/main.sv`:

```solveig
greet : String -> String
greet name = "Hello, " <> name <> "!"

main : Task ()
main = printLine (greet "Solveig")
```

Run it from any directory:

```sh
solveig --project-root path/to/hello
```

The Prelude is available automatically. Standard-library modules are explicit:

```solveig
import std.io(readFile)
import std.text(lines)
```

A bare import is qualified-only (`import std.text` gives
`text.trimAscii` and `std.text.trimAscii`). A precise list adds exactly those
unqualified names, while `import std.text(*)` deliberately opens the complete
visible API.

Library modules publish a deliberate external API while keeping unlisted
definitions reusable inside their package:

```solveig
import std.io(readFile)

export (Config, loadConfig)
private (validateRawConfig)
```

Use `Config(..)` when constructors are public, and build facade modules with
precise `reexport package.module(...)` declarations. The
[package and module reference](docs/packages.md) covers import qualification,
visibility, opaque types, re-exports, and path dependencies.

## A small language tour

Equations and patterns are ordinary ways to define functions:

```solveig
data Shape = Circle Float | Rectangle Float Float

area : Shape -> Float
area (Circle radius) = 3.14159 * radius * radius
area (Rectangle width height) = width * height

describe : Maybe Int -> String
describe Nothing = "no value"
describe (Just value) = "value: " <> show value
```

Effects are visible in types and sequenced with `do`:

```solveig
import std.io(readFile)

main : Task ()
main = do
    text <- readFile "message.txt"
    printStr text
```

Concurrent work can be scoped and bounded:

```solveig
import std.task(concurrentHostError, mapConcurrentlyBoundedOrdered)
import std.time(milliseconds, sleep)

work : Int -> Task Int
work value = do
    sleep (milliseconds 10)
    pureIO (value * value)

main : Task ()
main = do
    results <- mapError concurrentHostError (mapConcurrentlyBoundedOrdered 4 work [1, 2, 3, 4, 5, 6])
    printLine (show results)
```

At most four `work` actions run at once. Results retain input order even if the
actions finish in a different order.

## Documentation

Start with the [tutorial](docs/tutorial/README.md). It is written as a course
and includes runnable projects owned by the documentation itself.

- [Language basics](docs/tutorial/02-language-basics.md)
- [Data, patterns, and errors](docs/tutorial/03-data-and-patterns.md)
- [Functions, modules, and typeclasses](docs/tutorial/04-abstraction.md)
- [IO and files](docs/tutorial/05-io-and-files.md)
- [Concurrency](docs/tutorial/06-concurrency.md)
- [Networking and HTTP](docs/tutorial/07-networking-and-http.md)
- [Parsers and JSON](docs/tutorial/08-parsers-and-json.md)
- [Packages, CLI, and editor](docs/tutorial/09-packages-and-tooling.md)

Reference documentation:

- [Packages](docs/packages.md)
- [Prelude](docs/prelude.md)
- [Standard library](docs/standard-library.md)
- [IO](docs/io.md)
- [Strict streams](docs/streaming.md)
- [Errors and defects](docs/errors.md)
- [Concurrency](docs/concurrency.md)
- [HTTP and HTTPS](docs/http.md)
- [Formatting](docs/formatting.md)
- [Language server](docs/lsp.md)
- [Diagnostics](docs/diagnostics.md)

## Command line

```text
solveig [OPTIONS] [-- PROGRAM_ARGUMENTS...]
solveig lsp
solveig fmt [--check] [--stdin] [--width N] [PATH]...
```

Useful options:

- `--project-root <PATH>` chooses the directory containing `solveig.toml`.
- `--entry <NAME>` chooses a top-level definition in the manifest entry module;
  it defaults to `main`.
- `--verify-core` enables extra internal verification while compiling.
- Arguments after `--` are available through `std.env.getArgs`.

`solveig fmt` formats a local package or selected files. Use `solveig fmt
--check` in CI to report differences without writing them. The formatter
preserves comments, refuses parser errors and non-local package source, and has
one style option: line width, defaulting to 100.

There is no interactive REPL command in the current CLI.

## VS Code support

The extension provides diagnostics, hover types, completion, definition and
reference navigation, rename, code actions, semantic highlighting, inlay hints,
signature help, and formatting.

Build and install it from the checkout:

```sh
cd solveig_vscode
npm install
npx vsce package --out solveig.vsix
code --install-extension solveig.vsix --force
```

If the `solveig` command is not on VS Code's `PATH`, set
`solveig.server.path` to the executable's absolute path.

Run **Solveig: Format Document** for explicit formatting. VS Code's own
`editor.formatOnSave` setting also uses the Solveig formatter; configure the
maximum width with `solveig.formatting.lineWidth`.

## Current boundaries

Solveig packages currently use local path dependencies. There is no registry,
version solver, lockfile, or package-install command. Tuples support arities two
through eight. HTTP/HTTPS is a client API; a high-level web-server framework is
not part of `std`, although TCP listeners are available in `std.net`.

These boundaries are stated so examples remain honest about what the language
can do today.

## Contributing

Compiler changes should pass:

```sh
cargo build --workspace
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Implementation notes and benchmark policy live in `docs/` but are separate
from the language tutorial.
