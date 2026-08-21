# 1. Getting started

This chapter creates the smallest normal Solveig project and explains every
file in it.

## Install the command

Solveig is currently installed from source. You need Rust and Cargo:

```sh
git clone https://github.com/slavo23/solveig_lang.git
cd solveig_lang
cargo install --path . --force
```

The installed binary contains the Prelude and standard library. If you also
want editor navigation into their source, copy the library trees beside the
binary:

```sh
INSTALL_ROOT="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}"
mkdir -p "$INSTALL_ROOT/bin/lib/prelude" "$INSTALL_ROOT/bin/lib/std"
cp -R lib/prelude/. "$INSTALL_ROOT/bin/lib/prelude"
cp -R lib/std/. "$INSTALL_ROOT/bin/lib/std"
```

Check the result:

```sh
solveig --help
```

## Create a package

Make a directory named `hello`:

```text
hello/
├── solveig.toml
└── src/
    └── main.sv
```

The manifest identifies the package and its entry module:

```toml
[package]
name = "hello"
version = "0.1.0"

[language]
entry = "main"
```

Put this program in `src/main.sv`:

```solveig
greeting : String
greeting = "Hello from Solveig"

main : Task ()
main = printLine greeting
```

Run it:

```sh
solveig --project-root path/to/hello
```

The output is:

```text
Hello from Solveig
```

## Read the types

The colon introduces a type signature:

```solveig
greeting : String
```

`greeting` is a text value. `main` has a different kind of type:

```solveig
main : Task ()
```

`Task a` describes an action that can interact with the outside world and, on
success, produces an `a`. Here `a` is `()`, the unit value, because printing is
performed for its effect rather than a useful return value.

`Task` is not a command that runs immediately when mentioned. It is a value
describing an action. The runtime executes the action selected by the CLI entry
point.

## Definitions and application

A definition gives a name to an expression:

```solveig
double : Int -> Int
double number = number * 2

answer : Int
answer = double 21
```

Function application uses spaces: `double 21`. It binds more tightly than
operators, so `double 10 + 1` means `(double 10) + 1`.

Functions are curried. A two-argument signature:

```solveig
add : Int -> Int -> Int
```

means that `add 2` is itself a function waiting for one `Int`:

```solveig
add : Int -> Int -> Int
add left right = left + right

addTwo : Int -> Int
addTwo = add 2
```

## Imports

Small universal definitions such as `Maybe`, lists, arithmetic, `show`, and
`Task` come from the implicit Prelude. The larger standard library is explicit:

```solveig
import std.text(length, trimAscii)

cleanLength : String -> Int
cleanLength text = length (trimAscii text)
```

Import a whole module when several names make the code clearer:

```solveig
import std.text

cleanLength : String -> Int
cleanLength text = std.text.length (std.text.trimAscii text)
```

Qualified names are especially useful when two modules export the same name.

## What the runner looks for

`[language].entry` selects a source module, not a function. The default CLI
entry definition is named `main`. You can run another definition in that module:

```sh
solveig --project-root path/to/hello --entry greeting
```

The selected definition may be a pure value or an IO action. For applications,
`main : Task ()` is the conventional shape.

Next: [Language basics](02-language-basics.md).
