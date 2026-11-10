# 1. Run, change, and diagnose a program

The fastest way to learn a language is to operate a real program before taking
it apart. In this chapter you will run Log Audit, locate its package boundary,
make one visible change, and read one type error.

**Skip ahead:** if you already understand package manifests, expression
languages, and static type errors, run the checkpoint and continue with
[pure transformations](02-language-basics.md).

## Build the compiler

From the repository root:

```sh
cargo build
target/debug/solveig --help
```

You can use `cargo run --` in every command below; Cargo then builds the
compiler if necessary and forwards the remaining arguments to Solveig.

## Run Log Audit

```sh
cargo run -- --project-root examples/log_audit
```

The program prints the contents it wrote to `output/report.txt`:

```text
Solveig log audit
files analyzed: 4
files missing: 1
events: 14
info: 8
warnings: 3
errors: 3
```

This is a hermetic checkpoint. All input files are under
[`examples/log_audit/logs`](../../examples/log_audit/logs/); one requested
path is intentionally absent. No network access or machine-specific
configuration is involved.

## A package has a manifest and source tree

Log Audit has this shape:

```text
examples/log_audit/
├── solveig.toml
├── logs/
├── output/
└── src/
    └── main.sv
```

The manifest is small:

```toml
[package]
name = "log_audit"
version = "0.1.0"

[language]
entry = "main"
```

`--project-root` names the directory containing this manifest. The entry
module defaults to `src/main.sv`; the manifest selects the top-level entry
definition `main`. A project is not inferred from the shell's current
directory when you provide `--project-root`, so repository commands remain
repeatable.

## Read from the boundary inward

Start at the type of `main` in
[`src/main.sv`](../../examples/log_audit/src/main.sv):

```solveig
main : Task ()
main = do
    jobs <- newBoundedChan 2
    results <- newBoundedChan 2
    summary <- runAudit jobs results
    closeChan results
    let reportPath = requiredPath "output/report.txt"
    mapError fsToIO (std.fs.writeFile reportPath (renderReport summary))
    report <- mapError fsToIO (std.fs.readFile reportPath)
    printStr report
```

Read `Task ()` as “an action that may perform ordinary host IO, may fail with
`IOError`, and returns unit on success.” `Task a` is the public alias for
`IO IOError a`. The error parameter is not hidden by the runtime; APIs with a
more specific error type keep it until the program deliberately maps or
handles it.

`do` sequences actions. In `name <- action`, the action runs and its success
value is bound to `name` for the following statements. A `let` binding is
pure and does not run an effect. The last line determines the block's result.

Solveig is expression-oriented: `if`, `case`, `let`, and `do` produce
values. There is no separate statement language hidden inside a function body.

## Imports are explicit

The top of the file contains:

```solveig
import std.task(*)
import std.channel(*)
import std.fs(FsError(..))
import std.path(Path, pathFromString)
import std.text(stringToChars)
```

The Prelude is in scope automatically. Standard-library modules are not.

- `import std.path(Path, pathFromString)` adds exactly those visible names
  unqualified.
- `FsError(..)` imports the type and its public constructors.
- `import std.task(*)` deliberately opens the module's complete public API.
- `import std.text` by itself is qualified-only; use `text.lines` or
  `std.text.lines`.

Precise imports make dependencies visible. Wildcard imports are useful in an
application module that intentionally consumes most of a focused API, but
library interfaces generally benefit from explicit lists.

## Make a visible change

Copy the example to a scratch directory before experimenting:

```sh
cp -R examples/log_audit /tmp/solveig-log-audit
```

In `/tmp/solveig-log-audit/src/main.sv`, change the first string in
`renderReport` from `"Solveig log audit\n"` to
`"Daily service log audit\n"`, then run:

```sh
cargo run -- --project-root /tmp/solveig-log-audit
```

The first line changes; the counts do not. You changed presentation, while the
analysis and IO pipeline remained untouched. That separation is what the next
chapters will explain.

## Read a type error as a constraint report

Now make `reportLine` inconsistent:

```solveig
reportLine : String -> Int -> String
reportLine label value = appendString label value
```

`appendString` requires two `String` arguments, but the signature says
`value` is an `Int`. The compiler rejects the application at `value` and
reports the incompatible expected and actual types. It does not insert an
implicit numeric-to-text conversion. The checked implementation uses
`show value` explicitly:

```solveig
reportLine : String -> Int -> String
reportLine label value = appendString label (appendString (show value) "\n")
```

When a diagnostic is long, begin with the primary source span and the two types.
The notes explain where each constraint came from. See the
[diagnostics guide](../diagnostics.md) for import, exhaustiveness, effect, and
module errors.

## Formatting is part of the loop

Format the copied package and then verify it is stable:

```sh
cargo run -- fmt --project-root /tmp/solveig-log-audit
cargo run -- fmt --check --project-root /tmp/solveig-log-audit
```

The formatter parses before writing and refuses invalid source. It may choose
`(.)` or `($)` layouts already present in the source, but it does not change
program meaning.

## Checkpoint

Restore the correct `reportLine`, run the copied project, and confirm:

1. the command exits successfully;
2. the report contains four analyzed files and one missing file;
3. `output/report.txt` exactly matches terminal output;
4. `fmt --check` succeeds.

The repository test suite checks the canonical application against its expected
output. Your scratch copy lets you change it without modifying that baseline.

## Exercises

1. Change only the report labels, not the data model.
2. Add another existing log file to the copied project and update the number
   passed to `collectResults`. Predict which totals change before running it.
3. Misspell `renderReport` at its call site. Identify the primary diagnostic
   before reading any suggestions.

## Recap

- A Solveig project is rooted at `solveig.toml` and source lives under
  `src/`.
- Signatures describe both values and effect/error boundaries.
- Standard-library imports are explicit; the Prelude is automatic.
- `do` sequences effects, while `let` introduces a pure value.
- The compiler does not silently coerce `Int` to `String`.
- Run, diagnose, format, and rerun is the basic development loop.

Next: [Transform log data with pure functions](02-language-basics.md).
