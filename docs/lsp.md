# Solveig Language Server

Solveig editor support is provided by the `solveig lsp` command. Editors start
this command in the background and send it the text of open `.sv` files. The
server then answers editor requests such as diagnostics, hover, completion, and
go-to-definition.

## Current Features

- Diagnostics for parser, resolver, kind, type, hole, and core checks.
- Hover for functions, constructors, types, classes, methods, modules, local
  variables, function parameters, lambda parameters, `let` binders, do-block
  binders, case-pattern variables, literals, parenthesized expressions, `::`,
  and `[]`.
- Go-to-definition and find-references for resolved project declarations.
- Document outline and workspace symbol search.
- Semantic highlighting with full and delta token responses.
- Completion for locals, imports, project declarations, Prelude items, methods,
  constructors, modules, and keywords.
- Inlay hints for inferred signatures and inferred parameter/local types.
- Code actions for typed holes, missing imports, missing instance methods, and
  inferred signature insertion.
- Document highlights, folding ranges, selection expansion, signature help,
  rename, and clickable module imports.

## Correctness Plan

The server should be correct at three layers:

1. Source facts in `solveig-driver`.
   Driver APIs own semantic information in file spans and byte offsets. Tests
   should verify these facts directly before they are converted into any editor
   protocol shape.

2. LSP conversion in `solveig-lsp`.
   LSP tests should verify UTF-8/UTF-16 position conversion, protocol ranges,
   Markdown hover rendering, semantic-token delta encoding, workspace edits, and
   advertised capabilities.

3. Editor wiring in `solveig_vscode`.
   The VS Code extension should stay thin: it starts `solveig lsp`, contributes
   snippets and a shallow TextMate grammar, and avoids duplicating semantic
   language logic in JavaScript.

## Robustness Plan

The H23 robustness harness should grow in this order:

1. Token sweeps over real projects.
   For `examples/scry` and `examples/smoke_suite`, lex every user file and query
   hover, definition, references, document highlights, selection ranges,
   signature help, completion, semantic tokens, folding ranges, document links,
   and inlay hints. Assert that requests do not panic, spans are valid, semantic
   tokens do not overlap, generated helper names do not leak, and syntax-only
   tokens do not show misleading hover popups.

2. Focused golden snapshots.
   Pin token-by-token hover output for tricky areas, especially nested `case`
   expressions, nested patterns, do-blocks, imports, and qualified names. These
   snapshots make regressions readable.

3. Realistic editing sessions.
   Drive the in-process LSP server through open, change, save, and close cycles.
   Include temporarily broken source, incomplete `case` arms, partial imports,
   typed holes, and nested project roots. Assert no stale diagnostics remain
   after fixes and no request panics while the file is invalid.

4. Random edit survivability.
   Apply deterministic pseudo-random edits to small and medium source files.
   After each edit, request diagnostics plus a representative set of editor
   features. The pass condition is not "the edited program is valid"; it is "the
   server responds and diagnostics stay tied to current text."

5. Budget and growth checks.
   Run a long edit loop, around 1000 changes, and track retained source files,
   semantic-token result cache size, diagnostics, and resident editor state.
   The goal is bounded growth for a normal editing session.

## Findings From The First Sweep

The first `scry`, `smoke_suite`, and quiz-game sweeps found and fixed several
concrete issues:

- Hover on keywords and separators such as `case`, `of`, `=>`, and `fun`
  could show the type of a nearby expression. These now stay quiet.
- Hover inside a string such as `"/"` or `"> "` could be mistaken for an
  operator hover. String, char, number, and float literals now report their own
  literal type.
- Hover on opening or closing parentheses now reports the type of the
  expression inside those parentheses.
- Hover on `::` now reports the list constructor, and hover on `[]` now reports
  the empty-list constructor.
- Hover on import punctuation could show `module ...` because module hover used
  the whole import line. Module hover now uses only the module path span.
- Some repeated source-map entries caused editor lookup to choose a file id
  without retained semantic artifacts. File lookup now prefers artifact-backed
  file ids.
- Empty generated expression spans could produce zero-width hover ranges. Those
  generated spans are traversed but no longer offered as hover results.

## Unsupported Or Incomplete

- Formatting is not implemented yet.
- Rename is semantic for resolved declarations; local-variable rename is not a
  full feature yet.
- The TextMate grammar is intentionally shallow. It provides fallback coloring
  for comments, keywords, literals, declarations, and operators. Semantic
  highlighting from the language server provides the accurate coloring.
- The robustness harness currently sweeps driver APIs and selected in-process
  LSP paths. The next expansion is a longer LSP lifecycle test with many edits.
- Hover inside class declarations is still best for names and method
  signatures. Ordinary functions and explicit instance method bodies currently
  have more complete local-variable hover coverage.
