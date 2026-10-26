import type * as Monaco from "monaco-editor/editor/editor.api";

/** Syntax highlighting and editing behaviour for Nickel (.ncl). */
export function registerNickel(monaco: typeof Monaco): void {
  monaco.languages.register({ id: "nickel", extensions: [".ncl"], aliases: ["Nickel", "nickel"] });

  monaco.languages.setLanguageConfiguration("nickel", {
    comments: { lineComment: "#" },
    brackets: [
      ["{", "}"],
      ["[", "]"],
      ["(", ")"],
    ],
    autoClosingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"', notIn: ["string", "comment"] },
    ],
    surroundingPairs: [
      { open: "{", close: "}" },
      { open: "[", close: "]" },
      { open: "(", close: ")" },
      { open: '"', close: '"' },
    ],
    folding: { offSide: false },
    indentationRules: {
      increaseIndentPattern: /[{[(]\s*(#.*)?$/,
      decreaseIndentPattern: /^\s*[}\])]/,
    },
    wordPattern: /'?[A-Za-z_][A-Za-z0-9_\-']*|-?\d+(\.\d+)?/,
  });

  monaco.languages.setMonarchTokensProvider("nickel", {
    defaultToken: "",
    keywords: [
      "let",
      "in",
      "rec",
      "if",
      "then",
      "else",
      "fun",
      "match",
      "import",
      "forall",
      "default",
      "force",
      "priority",
      "optional",
      "doc",
      "not_exported",
      "include",
    ],
    constants: ["true", "false", "null"],
    types: ["Number", "String", "Bool", "Array", "Dyn", "Function"],
    tokenizer: {
      root: [
        [/#.*$/, "comment"],
        [/m(%+)"/, { token: "string.quote", next: "@multiline.$1" }],
        [/"/, { token: "string.quote", next: "@string" }],
        [/'[A-Za-z_][\w\-']*/, "enum"],
        [/'"/, { token: "enum", next: "@string" }],
        [/-?\d+(\.\d+)?([eE][+-]?\d+)?/, "number"],
        [/std(?=\.)/, "predefined"],
        [
          /[A-Za-z_][\w\-']*(?=\s*(\|[^>|]|=[^=>]))/,
          { cases: { "@keywords": "keyword", "@default": "field" } },
        ],
        [
          /[A-Za-z_][\w\-']*/,
          {
            cases: {
              "@keywords": "keyword",
              "@constants": "constant",
              "@types": "type",
              "@default": "identifier",
            },
          },
        ],
        [/[{}[\]()]/, "@brackets"],
        [/=>|->|==|!=|&&|\|\||\+\+|<=|>=|\|>|[=&|+\-*/<>@!.:,;%]/, "operator"],
        [/\s+/, "white"],
      ],
      string: [
        [/%\{/, { token: "delimiter.interpolation", next: "@interpolation" }],
        [/\\./, "string.escape"],
        [/"/, { token: "string.quote", next: "@pop" }],
        [/[^"\\%]+/, "string"],
        [/%/, "string"],
      ],
      multiline: [
        [/"(%+)/, { cases: { "$1==$S2": { token: "string.quote", next: "@pop" }, "@default": "string" } }],
        [/%+\{/, { token: "delimiter.interpolation", next: "@interpolation" }],
        [/[^"%]+/, "string"],
        [/["%]/, "string"],
      ],
      interpolation: [[/\}/, { token: "delimiter.interpolation", next: "@pop" }], { include: "root" }],
    },
  } as Monaco.languages.IMonarchLanguage);
}
