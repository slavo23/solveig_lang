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
  "qualified",
  "hiding",
];

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
    label: "signature",
    detail: "Type signature",
    insertText: new vscode.SnippetString("${1:name} : ${2:Type}"),
  },
  {
    label: "data",
    detail: "Data declaration",
    insertText: new vscode.SnippetString(
      "data ${1:Type} ${2:a} = ${3:Ctor} ${2:a}"
    ),
  },
  {
    label: "class",
    detail: "Typeclass declaration",
    insertText: new vscode.SnippetString(
      "class ${1:Class} ${2:a} where\n\t${3:method} : ${2:a} -> ${4:Result}"
    ),
  },
  {
    label: "instance",
    detail: "Typeclass instance",
    insertText: new vscode.SnippetString(
      "instance ${1:Class} ${2:Type} where\n\t${3:method} ${4:x} = ${5:expr}"
    ),
  },
  {
    label: "import",
    detail: "Import declaration",
    insertText: new vscode.SnippetString("import ${1:module}${2:(${3:name})}"),
  },
];

function makeKeywordItem(label) {
  const item = new vscode.CompletionItem(
    label,
    vscode.CompletionItemKind.Keyword
  );
  item.insertText = label;
  item.detail = "Solveig keyword";
  item.sortText = `1_${label}`;
  return item;
}

function makeSnippetItem(spec) {
  const item = new vscode.CompletionItem(
    spec.label,
    vscode.CompletionItemKind.Snippet
  );
  item.detail = spec.detail;
  item.insertText = spec.insertText;
  item.insertTextFormat = vscode.InsertTextFormat.Snippet;
  item.sortText = `0_${spec.label}`;
  return item;
}

function stripLineComment(line) {
  // This is intentionally lightweight. TextMate handles real lexical comments;
  // this only keeps symbol extraction from treating obvious line comments as code.
  const ix = line.indexOf("--");
  return ix === -1 ? line : line.slice(0, ix);
}

const FUNCTION_EQUATION_RE =
  /^\s*(?!let\b)([a-z_][A-Za-z0-9_']*)\b(?=[^\n]*(?<![=!<>])=(?![=>]))/;

function extractDeclaredClassName(line) {
  const match = stripLineComment(line).match(/^\s*class\b(.+?)(?:\bwhere\b|$)/);
  if (!match) {
    return undefined;
  }

  // In `class Eq a => Ord a where`, the declared class is after the last `=>`.
  const head = match[1].split("=>").pop();
  const classMatch = head.match(/\b([A-Z][A-Za-z0-9_']*)\b/);
  return classMatch?.[1];
}

function extractInstanceName(line) {
  const match = stripLineComment(line).match(
    /^\s*instance\b\s+(.+?)\s+where\b/
  );
  return match ? `instance ${match[1].trim()}` : undefined;
}

function collectSymbolsFromText(text) {
  const symbols = {
    types: new Set(),
    classes: new Set(),
    constructors: new Set(),
    functions: new Set(),
  };

  for (const rawLine of text.split(/\r?\n/)) {
    const line = stripLineComment(rawLine);

    const dataMatch = line.match(/^\s*data\s+([A-Z][A-Za-z0-9_']*)\b(.*)$/);
    if (dataMatch) {
      symbols.types.add(dataMatch[1]);

      const rhsIndex = line.indexOf("=");
      if (rhsIndex !== -1) {
        const rhs = line.slice(rhsIndex + 1);
        for (const ctorMatch of rhs.matchAll(
          /(?:^|\|)\s*([A-Z][A-Za-z0-9_']*)\b/g
        )) {
          symbols.constructors.add(ctorMatch[1]);
        }
      }
    }

    const className = extractDeclaredClassName(line);
    if (className) {
      symbols.classes.add(className);
    }

    const signatureMatch = line.match(/^\s*([a-z_][A-Za-z0-9_']*)\s*:/);
    if (signatureMatch) {
      symbols.functions.add(signatureMatch[1]);
    }

    const functionMatch = line.match(FUNCTION_EQUATION_RE);
    if (functionMatch) {
      symbols.functions.add(functionMatch[1]);
    }
  }

  return symbols;
}

function mergeSymbolSets(target, source) {
  for (const key of Object.keys(target)) {
    for (const value of source[key]) {
      target[key].add(value);
    }
  }
}

function collectWorkspaceSymbols(currentDocument) {
  const all = {
    types: new Set(),
    classes: new Set(),
    constructors: new Set(),
    functions: new Set(),
  };

  mergeSymbolSets(all, collectSymbolsFromText(currentDocument.getText()));

  for (const doc of vscode.workspace.textDocuments) {
    if (doc === currentDocument || doc.languageId !== LANGUAGE_ID) {
      continue;
    }

    mergeSymbolSets(all, collectSymbolsFromText(doc.getText()));
  }

  return all;
}

function makeDynamicItem(label, kind, detail, sortGroup) {
  const item = new vscode.CompletionItem(label, kind);
  item.insertText = label;
  item.detail = detail;
  item.sortText = `${sortGroup}_${label}`;
  return item;
}

function buildCompletionItems(document) {
  const symbols = collectWorkspaceSymbols(document);

  return [
    ...SNIPPETS.map(makeSnippetItem),

    ...Array.from(symbols.functions)
      .sort()
      .map((name) =>
        makeDynamicItem(
          name,
          vscode.CompletionItemKind.Function,
          "Function",
          "1"
        )
      ),

    ...Array.from(symbols.constructors)
      .sort()
      .map((name) =>
        makeDynamicItem(
          name,
          vscode.CompletionItemKind.Constructor,
          "Constructor",
          "1"
        )
      ),

    ...Array.from(symbols.classes)
      .sort()
      .map((name) =>
        makeDynamicItem(
          name,
          vscode.CompletionItemKind.Interface,
          "Typeclass",
          "1"
        )
      ),

    ...Array.from(symbols.types)
      .sort()
      .map((name) =>
        makeDynamicItem(name, vscode.CompletionItemKind.Class, "Type", "1")
      ),

    ...KEYWORDS.map(makeKeywordItem),
  ];
}

class SolveigCompletionProvider {
  provideCompletionItems(document, position) {
    const linePrefix = document
      .lineAt(position)
      .text.slice(0, position.character);

    const wordRange =
      document.getWordRangeAtPosition(
        position,
        /[A-Za-z_][A-Za-z0-9_']*/
      ) ?? undefined;

    return buildCompletionItems(document).map((item) => {
      if (wordRange) {
        item.range = wordRange;
      }

      if (/^\s*$/.test(linePrefix)) {
        item.preselect = item.kind === vscode.CompletionItemKind.Snippet;
      }

      return item;
    });
  }
}

class SolveigDocumentSymbolProvider {
  provideDocumentSymbols(document) {
    const symbols = [];
    const lines = document.getText().split(/\r?\n/);

    for (let lineIndex = 0; lineIndex < lines.length; lineIndex += 1) {
      const rawLine = lines[lineIndex];
      const line = stripLineComment(rawLine);

      let match = line.match(/^\s*data\s+([A-Z][A-Za-z0-9_']*)\b/);
      if (match) {
        symbols.push(
          makeLineSymbol(document, lineIndex, match[1], vscode.SymbolKind.Struct)
        );
        continue;
      }

      const className = extractDeclaredClassName(line);
      if (className) {
        symbols.push(
          makeLineSymbol(
            document,
            lineIndex,
            className,
            vscode.SymbolKind.Interface
          )
        );
        continue;
      }

      const instanceName = extractInstanceName(line);
      if (instanceName) {
        symbols.push(
          makeLineSymbol(
            document,
            lineIndex,
            instanceName,
            vscode.SymbolKind.Object
          )
        );
        continue;
      }

      match = line.match(/^\s*([a-z_][A-Za-z0-9_']*)\s*:/);
      if (match) {
        symbols.push(
          makeLineSymbol(
            document,
            lineIndex,
            match[1],
            vscode.SymbolKind.Interface
          )
        );
        continue;
      }

      match = line.match(FUNCTION_EQUATION_RE);
      if (match) {
        symbols.push(
          makeLineSymbol(
            document,
            lineIndex,
            match[1],
            vscode.SymbolKind.Function
          )
        );
      }
    }

    return symbols;
  }
}

function makeLineSymbol(document, lineIndex, name, kind) {
  const line = document.lineAt(lineIndex);
  const rawText = line.text;
  const selectableName = name.replace(/^instance\s+/, "");
  const nameStart = Math.max(0, rawText.indexOf(selectableName));
  const start = line.range.start.translate(0, nameStart);
  const end = line.range.end;
  const range = new vscode.Range(line.range.start, end);
  const selectionRange = new vscode.Range(
    start,
    start.translate(0, Math.max(1, selectableName.length))
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

function deactivate() { }

module.exports = {
  activate,
  deactivate,
};