# Solveig for Visual Studio Code

This extension adds editor support for `.sv` Solveig files.

## Install

Install the Solveig command first and make sure this works in a terminal:

```bash
solveig --help
```

Then install the extension from a packaged `.vsix` file:

```bash
code --install-extension solveig-0.1.0.vsix
```

After marketplace publishing, you can install it by extension id:

```bash
code --install-extension solveig-lang.solveig
```

## Uninstall

```bash
code --uninstall-extension solveig-lang.solveig
```

## Configure

If `solveig` is not on your PATH, set **Solveig: Server Path** in VS Code
settings to the full path of the Solveig executable.

Examples:

```json
{
  "solveig.server.path": "/Users/you/.cargo/bin/solveig"
}
```

On Windows, use the path to `solveig.exe`.

## Use

Open a folder containing Solveig source files and then open any `.sv` file.
The extension starts automatically.

Available editor features:

- errors and warnings in the Problems panel
- hover information
- go to definition
- find references
- rename symbol
- completion
- snippets
- document outline and workspace symbol search
- folding and selection expansion
- signature help
- module links for imports

Useful commands:

- **Solveig: Restart Server**
- **Solveig: Show Output**
- **Solveig: Show Trace**
- **Solveig: Show Type at Cursor**

If the status bar shows an error, run **Solveig: Show Output** for details.

## Command-Line Problems

The extension also contributes a `solveig` problem matcher for VS Code tasks
that run the Solveig command. Use it in `.vscode/tasks.json` like this:

```json
{
  "label": "check Solveig",
  "type": "shell",
  "command": "solveig --project-root .",
  "problemMatcher": "$solveig"
}
```

## Development

For server changes in this repository:

1. Build the debug binary:

   ```bash
   cargo build
   ```

2. In your workspace settings, point the extension at it:

   ```json
   {
     "solveig.server.path": "${workspaceFolder}/target/debug/solveig"
   }
   ```

3. Run **Solveig: Restart Server** after rebuilding.

For extension changes, open `solveig_vscode` in VS Code and press F5 to launch
an Extension Development Host.

For a local full install from this repository:

```bash
just install
```
