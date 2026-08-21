# Solveig language server

The `solveig lsp` command provides editor intelligence over the Language Server
Protocol. The bundled VS Code extension starts it automatically for `.sv`
files.

## Install the VS Code extension

First install the `solveig` executable as described in the
[README](../README.md). Then build the extension from the repository:

```sh
cd solveig_vscode
npm install
npx vsce package --out solveig.vsix
code --install-extension solveig.vsix --force
```

If VS Code cannot find `solveig` on its inherited `PATH`, set
`solveig.server.path` to the executable's absolute path. Use **Solveig: Restart
Server** after changing that setting.

## Editor features

The current server provides:

- diagnostics from parsing, name resolution, kind checking, type checking,
  exhaustiveness checking, typed holes, and core verification;
- hover types and documentation for declarations, locals, pattern binders,
  literals, tuple delimiters and elements, list syntax, and operators;
- completion for local bindings, imports, modules, declarations, constructors,
  methods, Prelude names, and keywords;
- go to definition, find references, document highlights, document symbols,
  workspace symbols, and clickable module imports;
- signature help, selection ranges, and folding ranges;
- semantic highlighting with incremental updates;
- optional inlay hints for inferred signatures and local types;
- code actions for typed holes, missing imports, missing instance methods, and
  inserting an inferred top-level signature;
- semantic rename for supported resolved declarations.

Formatting is not provided by the current language server.

## Project discovery

For an opened `.sv` file, the server walks upward and uses the nearest directory
containing `solveig.toml`. This makes nested packages independent even when a
broader parent directory is open in VS Code.

A bare file with no manifest is analyzed as a scratch project rooted at its
directory. Use a manifest for normal applications and libraries so entry
modules, dependencies, and package-qualified imports are unambiguous.

When a file changes in the editor, the server analyzes the in-memory text. You
do not have to save before diagnostics, hover, or completion update.

## Library source and edit ownership

Path dependencies, Prelude, and standard-library source are available for
navigation. Hover, completion, definition, and references can cross package
boundaries.

Editor-generated mutations are more conservative:

- rename is refused when completing it would require editing non-local source;
- code actions do not return edits for dependency or compiler-library files.

This is an IDE ownership rule, not a filesystem permission. You can update a
library on disk. Open an ordinary path dependency as its own project when you
want local refactors there. The shipped Prelude and `std` source remain
protected from generated edits and can be changed manually in a checkout or a
custom `SOLVEIG_LIB` tree.

## Settings

`solveig.server.path`
: Absolute path to the `solveig` executable. Leave empty to use `PATH`.

`solveig.inlayHints.enabled`
: Shows inline inferred types. It is off by default; hover and diagnostics are
  unaffected.

`solveig.diagnostics.inferredSignatures.enabled`
: Shows hints for top-level definitions without written signatures and offers
  an insertion action when available. It is on by default.

## Useful commands

- **Solveig: Restart Server** restarts the language-server process.
- **Solveig: Show Output** opens extension and server status output.
- **Solveig: Show Trace** opens the language-server trace log.
- **Solveig: Show Type at Cursor** requests the type at the active position.

By default the server trace file is `solveig-lsp.log` in the operating system's
temporary directory. Set `SOLVEIG_LSP_LOG` before starting VS Code to choose a
different path.

## Troubleshooting

**The extension activates but language features do not appear**

Run `solveig --help` in a terminal, then use **Solveig: Show Output**. If VS Code
has a different `PATH`, set `solveig.server.path` explicitly and restart the
server.

**Imports or dependencies are missing**

Check which `solveig.toml` is nearest to the open file. Confirm that dependency
paths are relative to that manifest and that imports use each dependency's
declared package name.

**Prelude or `std` navigation opens unexpected source**

Check `SOLVEIG_LIB`. It must point to the directory containing both
`prelude/solveig.toml` and `std/solveig.toml`. Without the override, an installed
`lib/` beside the executable wins; otherwise the binary's embedded source is
used.

**Rename is refused**

The declaration has references in source the current project does not own, or
the declaration itself is non-local. Open an ordinary dependency as its own
project or perform the cross-package edit manually.

**A hover is less detailed inside a class declaration**

Class names and method signatures have semantic hover. Local-variable hover is
most complete in ordinary function and explicit instance-method bodies.
