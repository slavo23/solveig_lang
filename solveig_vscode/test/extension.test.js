const assert = require("node:assert/strict");
const { EventEmitter } = require("node:events");
const Module = require("node:module");
const path = require("node:path");
const test = require("node:test");

test("manifest and activation expose formatting through VS Code", async () => {
  const manifest = require("../package.json");
  const registered = new Map();
  const executed = [];
  const information = [];
  const settings = {
    "formatting.lineWidth": 88,
  };
  const window = {
    activeTextEditor: {
      document: {
        languageId: "solveig",
      },
    },
    createOutputChannel: () => ({
      appendLine() {},
      dispose() {},
      show() {},
    }),
    createStatusBarItem: () => ({
      dispose() {},
      show() {},
    }),
    showErrorMessage: async () => undefined,
    showInformationMessage: (message) => {
      information.push(message);
    },
  };
  const vscode = {
    StatusBarAlignment: { Left: 1 },
    commands: {
      executeCommand: async (command) => {
        executed.push(command);
      },
      registerCommand: (command, callback) => {
        registered.set(command, callback);
        return { dispose() {} };
      },
    },
    window,
    workspace: {
      createFileSystemWatcher: () => ({ dispose() {} }),
      getConfiguration: () => ({
        get: (setting, fallback) => settings[setting] ?? fallback,
      }),
      onDidChangeConfiguration: () => ({ dispose() {} }),
      workspaceFolders: [
        {
          uri: {
            fsPath: path.resolve(".."),
          },
        },
      ],
    },
  };
  let clientOptions;
  class LanguageClient {
    constructor(_id, _name, _serverOptions, options) {
      clientOptions = options;
      this.state = 2;
    }

    onDidChangeState() {}

    async start() {}

    async stop() {}
  }
  const childProcess = {
    spawn: () => {
      const child = new EventEmitter();
      process.nextTick(() => child.emit("exit", 0));
      return child;
    },
  };
  const originalLoad = Module._load;
  Module._load = function load(request, parent, isMain) {
    if (request === "vscode") {
      return vscode;
    }
    if (request === "vscode-languageclient/node") {
      return {
        LanguageClient,
        RevealOutputChannelOn: { Never: 0 },
        State: { Starting: 1, Running: 2, Stopped: 3 },
      };
    }
    if (request === "child_process") {
      return childProcess;
    }
    return originalLoad(request, parent, isMain);
  };

  let extension;
  try {
    extension = require("../extension.js");
  } finally {
    Module._load = originalLoad;
  }
  const context = {
    subscriptions: [],
  };
  extension.activate(context);
  await new Promise((resolve) => setImmediate(resolve));

  assert.ok(manifest.activationEvents.includes("onCommand:solveig.formatDocument"));
  assert.equal(
    manifest.contributes.configuration.properties["solveig.formatting.lineWidth"].default,
    100
  );
  assert.ok(registered.has("solveig.formatDocument"));
  assert.equal(clientOptions.initializationOptions.formatting.lineWidth, 88);
  assert.equal(clientOptions.middleware, undefined);
  assert.deepEqual(clientOptions.documentSelector, [
    { language: "solveig", scheme: "file" },
    { language: "solveig", scheme: "untitled" },
  ]);

  await registered.get("solveig.formatDocument")();
  assert.deepEqual(executed, ["editor.action.formatDocument"]);

  window.activeTextEditor.document.languageId = "plaintext";
  await registered.get("solveig.formatDocument")();
  assert.deepEqual(executed, ["editor.action.formatDocument"]);
  assert.deepEqual(information, ["Open a Solveig file first."]);

  await extension.deactivate();
});
