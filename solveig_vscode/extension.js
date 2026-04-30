const vscode = require("vscode");

const LANGUAGE_ID = "solveig";
const DOCUMENT_SELECTOR = [
  { language: LANGUAGE_ID, scheme: "file" },
  { language: LANGUAGE_ID, scheme: "untitled" },
];

const KEYWORDS = [
  "case",
  "of",
  "if",
  "then",
  "else",
  "let",
  "in",
  "where",
  "fun",
  "forall",
  "data",
  "sig",
  "class",
  "instance",
  "import",
  "as",
];

const BUILTIN_TYPES = ["Bool", "Int", "Pair", "List"];
const BUILTIN_CONSTRUCTORS = ["True", "False", "Nil", "Cons"];

const SNIPPETS = [
  {
    label: "case",
    detail: "Case expression",
    insertText: new vscode.SnippetString(
      "case ${1:expr} of\n\t${2:Pattern} => ${3:expr}"
    ),
  },
  {
    label: "if",
    detail: "If expression",
    insertText: new vscode.SnippetString(
      "if ${1:cond} then\n\t${2:thenExpr}\nelse\n\t${3:elseExpr}"
    ),
  },
  {
    label: "fun",
    detail: "Lambda",
    insertText: new vscode.SnippetString("fun ${1:x} => ${2:expr}"),
  },
  {
    label: "let",
    detail: "Let expression",
    insertText: new vscode.SnippetString(
      "let ${1:name} = ${2:expr} in\n\t${3:body}"
    ),
  },
  {
    label: "sig",
    detail: "Type signature",
    insertText: new vscode.SnippetString("${1:name} : ${2:Type}"),
  },
  {
    label: "data",
    detail: "Data declaration",
    insertText: new vscode.SnippetString(
      "data ${1:Type} = ${2:Ctor}${3: | ${4:OtherCtor}}"
    ),
  },
  {
    label: "import",
    detail: "Import declaration",
    insertText: new vscode.SnippetString(
      "import ${1:Module}${2: as ${3:Alias}}"
    ),
  },
];

function makeKeywordItem(label) {
  const item = new vscode.CompletionItem(label, vscode.CompletionItemKind.Keyword);
  item.insertText = label;
  item.detail = "Solveig keyword";
  return item;
}

function makeTypeItem(label) {
  const item = new vscode.CompletionItem(label, vscode.CompletionItemKind.Class);
  item.insertText = label;
  item.detail = "Built-in type";
  return item;
}

function makeConstructorItem(label) {
  const item = new vscode.CompletionItem(label, vscode.CompletionItemKind.EnumMember);
  item.insertText = label;
  item.detail = "Built-in constructor";
  return item;
}

function makeSnippetItem(spec) {
  const item = new vscode.CompletionItem(spec.label, vscode.CompletionItemKind.Snippet);
  item.detail = spec.detail;
  item.insertText = spec.insertText;
  item.insertTextFormat = vscode.InsertTextFormat.Snippet;
  item.sortText = `0_${spec.label}`;
  return item;
}

function buildCompletionItems() {
  return [
    ...SNIPPETS.map(makeSnippetItem),
    ...KEYWORDS.map(makeKeywordItem),
    ...BUILTIN_TYPES.map(makeTypeItem),
    ...BUILTIN_CONSTRUCTORS.map(makeConstructorItem),
  ];
}

class SolveigCompletionProvider {
  constructor() {
    this.items = buildCompletionItems();
  }

  provideCompletionItems(document, position) {
    const linePrefix = document.lineAt(position).text.slice(0, position.character);
    const wordRange =
      document.getWordRangeAtPosition(position, /[A-Za-z_][A-Za-z0-9_']*/) ?? undefined;

    return this.items.map((item) => {
      const copy = new vscode.CompletionItem(item.label, item.kind);
      copy.detail = item.detail;
      copy.insertText = item.insertText;
      copy.insertTextFormat = item.insertTextFormat;
      copy.sortText = item.sortText ?? item.label.toString();

      if (wordRange) {
        copy.range = wordRange;
      }

      if (/^\s*$/.test(linePrefix)) {
        copy.preselect = item.kind === vscode.CompletionItemKind.Snippet;
      }

      return copy;
    });
  }
}

class SolveigDocumentSymbolProvider {
  provideDocumentSymbols(document) {
    const symbols = [];
    const lines = document.getText().split(/\r?\n/);

    for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
      const line = lines[lineIndex];

      let match = line.match(/^\s*data\s+([A-Z][A-Za-z0-9_']*)\b/);
      if (match) {
        const name = match[1];
        symbols.push(makeLineSymbol(document, lineIndex, name, vscode.SymbolKind.Struct));
        continue;
      }

      match = line.match(/^\s*sig\s+([a-z_][A-Za-z0-9_']*)\b/);
      if (match) {
        const name = match[1];
        symbols.push(makeLineSymbol(document, lineIndex, name, vscode.SymbolKind.Interface));
        continue;
      }

      match = line.match(/^\s*([a-z_][A-Za-z0-9_']*)\b(?=.*=)/);
      if (match && !line.includes("=>")) {
        const name = match[1];
        symbols.push(makeLineSymbol(document, lineIndex, name, vscode.SymbolKind.Function));
        continue;
      }
    }

    return symbols;
  }
}

function makeLineSymbol(document, lineIndex, name, kind) {
  const line = document.lineAt(lineIndex);
  const start = line.range.start;
  const end = line.range.end;
  const range = new vscode.Range(start, end);
  const selectionRange = new vscode.Range(
    start,
    start.translate(0, name.length)
  );
  return new vscode.DocumentSymbol(name, "", kind, range, selectionRange);
}

function activate(context) {
  const completionProvider = vscode.languages.registerCompletionItemProvider(
    DOCUMENT_SELECTOR,
    new SolveigCompletionProvider(),
    "."
  );

  const documentSymbolProvider = vscode.languages.registerDocumentSymbolProvider(
    DOCUMENT_SELECTOR,
    new SolveigDocumentSymbolProvider()
  );

  context.subscriptions.push(completionProvider, documentSymbolProvider);
}

function deactivate() {}

module.exports = {
  activate,
  deactivate,
};
