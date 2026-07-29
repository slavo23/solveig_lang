# Solveig for Visual Studio Code

This extension adds editor support for `.sv` Solveig files.

The extension does not include the Solveig compiler. Install the `solveig`
command first, then install this extension. When you open a `.sv` file, the
extension starts `solveig` automatically.

## Requirements

You need:

- Visual Studio Code
- the `solveig` command
- VS Code's `code` command if you want to install from a terminal

If `code --version` does not work, open VS Code, run **Shell Command: Install
'code' command in PATH** from the Command Palette, or use the Extensions view
to install a `.vsix` file.

## Install Solveig

If you already have `solveig` installed, check it:

```bash
solveig --help
```

If that command works, continue to **Install the Extension**.

To install Solveig from the repository:

```bash
cargo install --git https://github.com/slavo23/lang_experiments.git
```

To install Solveig from a local checkout instead:

```bash
git clone https://github.com/slavo23/lang_experiments.git
cd lang_experiments
cargo install --path .
```

Cargo usually installs the binary to `~/.cargo/bin/solveig` on macOS and Linux,
or `%USERPROFILE%\.cargo\bin\solveig.exe` on Windows.

If `solveig --help` still says the command cannot be found, add Cargo's bin
directory to your PATH. On macOS or Linux:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

For a permanent setup, add that line to your shell profile.

## Install the Extension

After marketplace publishing, install the extension with:

```bash
code --install-extension solveig-lang.solveig
```

Until marketplace publishing is available, install a packaged `.vsix` file:

```bash
code --install-extension /path/to/solveig-0.1.0.vsix
```

You can also install a `.vsix` from inside VS Code: open the Extensions view,
open the `...` menu, choose **Install from VSIX...**, and select the file.

To build the `.vsix` from this repository:

```bash
cd solveig_vscode
npm install
npm run package -- --out /tmp/solveig.vsix
code --install-extension /tmp/solveig.vsix --force
```

## Configure

By default, the extension looks for `solveig` on your PATH. If `solveig --help`
works in the same environment that launches VS Code, no extra setting is
needed.

If VS Code cannot find `solveig`, set **Solveig: Server Path** to the full path
of the executable.

macOS or Linux example:

```json
{
  "solveig.server.path": "/Users/you/.cargo/bin/solveig"
}
```

Windows example:

```json
{
  "solveig.server.path": "C:\\Users\\you\\.cargo\\bin\\solveig.exe"
}
```

After changing the setting, run **Solveig: Restart Server** from the Command
Palette.

## Use

Open a folder containing Solveig source files, then open any `.sv` file. The
extension starts automatically and shows a Solveig item in the status bar.

You should then get:

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

If the status bar shows an error, run **Solveig: Show Output**. The most common
fix is either installing the `solveig` command or setting **Solveig: Server
Path** to the correct executable.

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

## Update

To update the Solveig command after installing with Cargo, run the same install
command again:

```bash
cargo install --git https://github.com/slavo23/lang_experiments.git --force
```

For a local checkout:

```bash
git pull
cargo install --path . --force
```

To update the extension from a new `.vsix`, install the new file with `--force`:

```bash
code --install-extension /path/to/solveig-0.1.0.vsix --force
```

## Uninstall

Uninstall the VS Code extension:

```bash
code --uninstall-extension solveig-lang.solveig
```

You can also uninstall from the Extensions view in VS Code.

Uninstall the Solveig command:

```bash
cargo uninstall solveig
```

If you installed Solveig by manually copying a binary, delete that copied
binary from the directory where you placed it. If you only ran `cargo build`,
nothing was installed system-wide.

## Development

For contributor testing in this repository, build the debug binary:

```bash
cargo build
```

Open `solveig_vscode` in VS Code and press F5. In the Extension Development
Host, open this repository or a Solveig project and set **Solveig: Server
Path** to the debug binary:

```json
{
  "solveig.server.path": "/Users/gorborukovstanislav/Desktop/code/rust/lang_experiments/target/debug/solveig"
}
```

If the Extension Development Host opens the repository root as its workspace,
this also works:

```json
{
  "solveig.server.path": "${workspaceFolder}/target/debug/solveig"
}
```

After rebuilding with `cargo build`, run **Solveig: Restart Server**.

For a local full install from this repository:

```bash
just install
```

To remove that local install:

```bash
just uninstall
cargo uninstall solveig
```
