const cp = require("child_process");
const os = require("os");
const path = require("path");
const vscode = require("vscode");
const {
  LanguageClient,
  RevealOutputChannelOn,
  State,
} = require("vscode-languageclient/node");

const LANGUAGE_ID = "solveig";
const SERVER_ARGS = ["lsp"];
const CONFIG_SECTION = "solveig";
const SERVER_PATH_SETTING = "server.path";

let client;
let outputChannel;
let traceChannel;
let statusBar;
let clientStarting = false;

function activate(context) {
  outputChannel = vscode.window.createOutputChannel("Solveig");
  traceChannel = vscode.window.createOutputChannel("Solveig Trace");
  statusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
  statusBar.command = "solveig.showOutput";

  context.subscriptions.push(outputChannel, traceChannel, statusBar);
  context.subscriptions.push(
    vscode.commands.registerCommand("solveig.restartServer", () =>
      restartClient(context)
    ),
    vscode.commands.registerCommand("solveig.showOutput", () =>
      outputChannel.show(true)
    ),
    vscode.commands.registerCommand("solveig.showTrace", () =>
      traceChannel.show(true)
    ),
    vscode.commands.registerCommand("solveig.showTypeAtCursor", showTypeAtCursor),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (event.affectsConfiguration(`${CONFIG_SECTION}.${SERVER_PATH_SETTING}`)) {
        restartClient(context);
      }
    })
  );

  updateStatus("starting");
  startClient(context);
}

async function deactivate() {
  if (!client) {
    return undefined;
  }

  const stopping = client.stop();
  client = undefined;
  return stopping;
}

async function restartClient(context) {
  updateStatus("starting");

  if (client) {
    const oldClient = client;
    client = undefined;
    await oldClient.stop();
  }

  return startClient(context);
}

async function startClient(context) {
  if (clientStarting) {
    return;
  }

  clientStarting = true;
  const server = resolveServerExecutable();

  try {
    await ensureServerStarts(server);
  } catch (error) {
    clientStarting = false;
    updateStatus("error");
    showServerStartError(server, error);
    return;
  }

  outputChannel.appendLine(`Starting Solveig server: ${server.command} ${SERVER_ARGS.join(" ")}`);

  const serverOptions = {
    command: server.command,
    args: SERVER_ARGS,
    options: {
      cwd: server.cwd,
    },
  };
  const clientOptions = {
    documentSelector: [
      { language: LANGUAGE_ID, scheme: "file" },
      { language: LANGUAGE_ID, scheme: "untitled" },
    ],
    outputChannel,
    traceOutputChannel: traceChannel,
    revealOutputChannelOn: RevealOutputChannelOn.Never,
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.sv"),
    },
  };

  client = new LanguageClient(
    "solveig",
    "Solveig",
    serverOptions,
    clientOptions
  );
  context.subscriptions.push(client);
  client.onDidChangeState((event) => updateStatusForClientState(event.newState));

  try {
    await client.start();
    updateStatusForClientState(client.state);
  } catch (error) {
    updateStatus("error");
    outputChannel.appendLine(`Failed to start Solveig server: ${errorMessage(error)}`);
    vscode.window.showErrorMessage(
      "Could not start the Solveig language server. Check the Solveig output for details."
    );
  } finally {
    clientStarting = false;
  }
}

function resolveServerExecutable() {
  const config = vscode.workspace.getConfiguration(CONFIG_SECTION);
  const configuredPath = config.get(SERVER_PATH_SETTING, "").trim();
  const workspaceFolder = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
  const cwd = workspaceFolder ?? process.cwd();

  if (!configuredPath) {
    return { command: "solveig", cwd, configured: false };
  }

  return {
    command: expandConfiguredPath(configuredPath, workspaceFolder),
    cwd,
    configured: true,
  };
}

function expandConfiguredPath(configuredPath, workspaceFolder) {
  let expanded = configuredPath;

  if (workspaceFolder) {
    expanded = expanded
      .replaceAll("${workspaceFolder}", workspaceFolder)
      .replaceAll("${workspaceRoot}", workspaceFolder);
  }

  if (expanded === "~") {
    return os.homedir();
  }

  if (expanded.startsWith(`~${path.sep}`) || expanded.startsWith("~/")) {
    return path.join(os.homedir(), expanded.slice(2));
  }

  return expanded;
}

function ensureServerStarts(server) {
  return new Promise((resolve, reject) => {
    const child = cp.spawn(server.command, ["--help"], {
      cwd: server.cwd,
      windowsHide: true,
    });

    child.once("error", reject);
    child.once("exit", (code) => {
      if (code === 0) {
        resolve();
      } else {
        reject(new Error(`'${server.command} --help' exited with code ${code}`));
      }
    });
  });
}

function showServerStartError(server, error) {
  const source = server.configured
    ? `the configured path '${server.command}'`
    : "'solveig' on your PATH";
  const message =
    `Could not start the Solveig language server using ${source}. ` +
    "Install Solveig or set 'Solveig: Server Path' to the Solveig executable.";

  outputChannel.appendLine(message);
  outputChannel.appendLine(errorMessage(error));
  vscode.window.showErrorMessage(message, "Open Settings", "Show Output").then((choice) => {
    if (choice === "Open Settings") {
      vscode.commands.executeCommand(
        "workbench.action.openSettings",
        `${CONFIG_SECTION}.${SERVER_PATH_SETTING}`
      );
    } else if (choice === "Show Output") {
      outputChannel.show(true);
    }
  });
}

function updateStatusForClientState(state) {
  switch (state) {
    case State.Running:
      updateStatus("running");
      break;
    case State.Starting:
      updateStatus("starting");
      break;
    case State.Stopped:
      updateStatus("stopped");
      break;
    default:
      updateStatus("stopped");
      break;
  }
}

function updateStatus(status) {
  if (!statusBar) {
    return;
  }

  switch (status) {
    case "running":
      statusBar.text = "$(check) Solveig";
      statusBar.tooltip = "Solveig language server is running";
      break;
    case "starting":
      statusBar.text = "$(sync~spin) Solveig";
      statusBar.tooltip = "Solveig language server is starting";
      break;
    case "error":
      statusBar.text = "$(error) Solveig";
      statusBar.tooltip = "Solveig language server could not start";
      break;
    case "stopped":
    default:
      statusBar.text = "$(circle-slash) Solveig";
      statusBar.tooltip = "Solveig language server is stopped";
      break;
  }

  statusBar.show();
}

async function showTypeAtCursor() {
  const editor = vscode.window.activeTextEditor;
  if (!editor || editor.document.languageId !== LANGUAGE_ID) {
    vscode.window.showInformationMessage("Open a Solveig file first.");
    return;
  }

  const hovers = await vscode.commands.executeCommand(
    "vscode.executeHoverProvider",
    editor.document.uri,
    editor.selection.active
  );
  const text = formatHoverContents(hovers);

  if (!text) {
    vscode.window.showInformationMessage("No Solveig type information at the cursor.");
    return;
  }

  outputChannel.appendLine("");
  outputChannel.appendLine(`Type at ${editor.document.fileName}:${editor.selection.active.line + 1}`);
  outputChannel.appendLine(text);
  outputChannel.show(true);
}

function formatHoverContents(hovers) {
  if (!Array.isArray(hovers)) {
    return "";
  }

  return hovers
    .flatMap((hover) => hover.contents ?? [])
    .map(markedContentToString)
    .filter(Boolean)
    .join("\n\n");
}

function markedContentToString(content) {
  if (typeof content === "string") {
    return content;
  }

  if (content?.language && content?.value) {
    return `\`\`\`${content.language}\n${content.value}\n\`\`\``;
  }

  if (content?.value) {
    return content.value;
  }

  return "";
}

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}

module.exports = {
  activate,
  deactivate,
};
