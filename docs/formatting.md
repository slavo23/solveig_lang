# Formatting Solveig source

Solveig has one canonical formatter shared by the command line and language
server. It reads the lossless concrete syntax tree, including comments, and
renders source with a width-aware layout. The only style option is maximum line
width, which defaults to 100 columns.

## The canonical style

The formatter uses four-space indentation and spaces around type signatures,
arrows, binds, definitions, and infix operators. It keeps `do` statements and
class or instance declarations on separate lines, collapses repeated blank
lines to one blank line, preserves every comment, and ends a complete file with
one newline.

Imports form one compact block followed by exactly one blank line before the
first declaration or its leading comments. When a function type must wrap, all
continuation arrows use the same four-space indentation instead of drifting
rightward. Broken tuples and lists place their contents one level inside
aligned opening and closing delimiters; ordinary grouping parentheses stay
compact, while a multiline lambda or layout expression gets an aligned closing
parenthesis.

For example:

```solveig
render:forall a.(Show a,Eq a)=>a->String
render value=show value

main=do printLine(render 42);pure ()
```

becomes:

```solveig
render : forall a. (Show a, Eq a) => a -> String
render value = show value

main = do
    printLine (render 42)
    pure ()
```

When a construct does not fit within the selected width, the formatter chooses
structural line breaks and indentation. A line can exceed the width only when a
single indivisible token, such as one string literal, is itself longer than the
limit.

There are no settings for tabs, brace placement, operator spacing, import
layout, or other stylistic choices. This keeps formatted Solveig source
consistent across projects and editors.

## Format files from the command line

Format the nearest local package:

```sh
solveig fmt
```

`solveig fmt` finds the package selected by `--project-root` and formats all of
its local modules. It does not format loaded path dependencies, Prelude, or the
standard library as part of that package.

Choose files or directories explicitly when you want a narrower operation:

```sh
solveig fmt src/main.sv src/report
```

Directories are searched recursively for `.sv` files. Selected paths are
canonicalized, deduplicated, and processed in deterministic path order. A
standalone `.sv` file can be formatted without a manifest:

```sh
solveig fmt scratch.sv
```

Set another line width with the formatter's sole option:

```sh
solveig fmt --width 88
solveig fmt --width 88 src
```

A width must be greater than zero.

### Standard input

Use `--stdin` when another tool owns the input and output streams:

```sh
solveig fmt --stdin < before.sv > after.sv
```

Standard-input mode formats one source and does not load a package. It writes
formatted source to standard output and diagnostics to standard error.

### Parse errors and write safety

The formatter does not guess through syntax errors. If any selected file has a
parser error, the command reports normal Solveig diagnostics, returns a failure
status, and leaves every selected file unchanged. Permissions and all parses
are validated before the first write, so a multi-file operation is not partly
applied.

## Check formatting in CI

`--check` reports formatting differences without changing files:

```sh
solveig fmt --check
```

The command exits successfully when all selected sources are already in
canonical form. It prints a deterministic unified-style diff and exits
unsuccessfully when formatting would change a file. It also exits
unsuccessfully for invalid input or a refused source.

A typical CI step needs only:

```sh
solveig fmt --check --project-root .
```

You can combine `--check` with explicit paths, `--width`, or `--stdin`:

```sh
solveig fmt --check --width 88 src
solveig fmt --check --stdin < generated.sv
```

If `--check` is clean, applying the formatter would make no textual change.
Formatter development is additionally gated by round-trip, idempotence,
comment-preservation, and width properties over the repository corpus and
generated valid programs. Formatted output must parse as the same trivia-free
surface program, and formatting it again must produce identical text.
Consequently, a clean `--check` result is round-trip safe: the source already
is the canonical output whose parsing and idempotence invariants gate the
formatter release.

## Visual Studio Code

The Solveig extension delegates formatting to `solveig lsp`; it does not carry
a JavaScript formatter. Open a Solveig file and run **Solveig: Format Document**
from the Command Palette. VS Code's standard **Format Document** action uses the
same provider.

The extension exposes the one formatter option as:

```json
{
  "solveig.formatting.lineWidth": 100
}
```

Changing it restarts the language server so the next formatting request uses
the new value.

### Format on save

Format-on-save is a VS Code editor setting. The Solveig extension does not
implement another save hook. A language-scoped configuration is:

```json
{
  "[solveig]": {
    "editor.defaultFormatter": "solveig-lang.solveig",
    "editor.formatOnSave": true
  },
  "solveig.formatting.lineWidth": 100
}
```

With this setting, VS Code invokes the same document-formatting request before
saving. If the current buffer has a parser error, the server returns no edits
and the save itself remains under VS Code's control.

### Format a selection

VS Code's **Format Selection** action uses Solveig's range-formatting request.
Because Solveig is layout-sensitive, the server does not format arbitrary token
fragments. A selection touching exactly one top-level definition formats that
complete definition. A selection touching two definitions, or only whitespace
between definitions, returns no edit. Leading comments and surrounding source
remain outside the replacement.

## Other LSP clients

The server advertises `textDocument/formatting` and
`textDocument/rangeFormatting`. It does not advertise on-type formatting.

Clients can configure the default width during initialization:

```json
{
  "formatting": {
    "lineWidth": 88
  }
}
```

An LSP formatting request may also include a positive numeric `lineWidth`
property in `FormattingOptions`; that request-specific value takes precedence
over the initialized default. Clients that send neither use 100.

## Package ownership

Formatting is an editor-generated mutation and follows the same ownership rule
as rename and code actions. The server formats local package source and refuses
path-dependency, Prelude, and standard-library files in the current project.
Those files remain readable and navigable, and the rule is not a filesystem
permission: open an ordinary path dependency as its own local project when you
want to format it, or edit library source manually in the checkout that owns
it.

## Troubleshooting

**VS Code says no formatter is available**

Confirm the Solveig status item says the server is running. Run **Solveig: Show
Output**, check that `solveig --help` works in the environment that launches VS
Code, and set `solveig.server.path` if needed. If several formatters claim `.sv`
files, set `editor.defaultFormatter` for `[solveig]` as shown above.

**Format Document makes no change**

The document may already be canonical, contain a parser error, or belong to a
non-local package. Check the Problems panel and Solveig output. For a selection,
make sure it touches exactly one top-level definition.

**CI and the editor disagree about width**

Pass the same numeric value to `solveig fmt --width` and
`solveig.formatting.lineWidth`. The formatter implementation and style are
otherwise identical.
