import * as monaco from "monaco-editor/editor/editor.api";
import "monaco-editor/features/register.all";
import EditorWorker from "monaco-editor/editor/editor.worker?worker";

import * as api from "../api";
import type { Diagnostic, Span } from "../types";
import { registerNickel } from "./nickel";
import { registerSchema } from "./schema";
import type { TextChange } from "./shift";

self.MonacoEnvironment = { getWorker: () => new EditorWorker() };

registerNickel(monaco);
registerSchema(monaco);

monaco.editor.defineTheme("plate-desk", {
  base: "vs-dark",
  inherit: true,
  rules: [
    { token: "comment", foreground: "6E767F", fontStyle: "italic" },
    { token: "keyword", foreground: "8FB3D9" },
    { token: "constant", foreground: "8FB3D9" },
    { token: "type", foreground: "7FC4B8" },
    { token: "field", foreground: "E6E3D8" },
    { token: "identifier", foreground: "C9CED4" },
    { token: "predefined", foreground: "7FC4B8" },
    { token: "enum", foreground: "F08C78" },
    { token: "number", foreground: "E2BE73" },
    { token: "string", foreground: "A9C99A" },
    { token: "string.quote", foreground: "A9C99A" },
    { token: "string.escape", foreground: "E2BE73" },
    { token: "delimiter.interpolation", foreground: "F08C78" },
    { token: "operator", foreground: "9AA2AB" },
  ],
  colors: {
    "editor.background": "#1A1D21",
    "editor.foreground": "#C9CED4",
    "editorLineNumber.foreground": "#4A5058",
    "editorLineNumber.activeForeground": "#B6BCC3",
    "editor.lineHighlightBackground": "#22262B",
    "editor.lineHighlightBorder": "#00000000",
    "editor.selectionBackground": "#3A4D63",
    "editor.inactiveSelectionBackground": "#2E3A48",
    "editorCursor.foreground": "#F08C78",
    "editorIndentGuide.background1": "#2A2E34",
    "editorIndentGuide.activeBackground1": "#3E444C",
    "editorBracketMatch.background": "#3A4D6355",
    "editorBracketMatch.border": "#5D7590",
    "editorError.foreground": "#F0705C",
    "editorWarning.foreground": "#E2BE73",
    "editorWidget.background": "#22262B",
    "editorWidget.border": "#363C43",
    "editorHoverWidget.background": "#22262B",
    "editorHoverWidget.border": "#363C43",
    "editorSuggestWidget.background": "#22262B",
    "editorSuggestWidget.border": "#363C43",
    "editorSuggestWidget.selectedBackground": "#2F3A46",
    "scrollbarSlider.background": "#3A404866",
    "scrollbarSlider.hoverBackground": "#4A5058AA",
    "editorGutter.background": "#1A1D21",
    "editorOverviewRuler.border": "#00000000",
    "editorBracketHighlight.foreground1": "#9AA2AB",
    "editorBracketHighlight.foreground2": "#9AA2AB",
    "editorBracketHighlight.foreground3": "#9AA2AB",
    "editorBracketHighlight.foreground4": "#9AA2AB",
    "editorBracketHighlight.foreground5": "#9AA2AB",
    "editorBracketHighlight.foreground6": "#9AA2AB",
  },
});

monaco.languages.registerDocumentFormattingEditProvider("nickel", {
  async provideDocumentFormattingEdits(model) {
    const result = await api.format(model.getValue());
    if (!result.ok) return [];
    return [{ range: model.getFullModelRange(), text: result.source }];
  },
});

export interface EditorCommands {
  save(): void;
  open(): void;
}

/** An editor for one group of tabs: the text it shows is whichever document's model is set. */
export function createCodeEditor(
  container: HTMLElement,
  commands: EditorCommands,
): monaco.editor.IStandaloneCodeEditor {
  const editor = monaco.editor.create(container, {
    model: null,
    theme: "plate-desk",
    automaticLayout: true,
    fontFamily: '"JetBrains Mono Variable", "Cascadia Code", Consolas, monospace',
    fontSize: 13.5,
    lineHeight: 22,
    fontLigatures: true,
    minimap: { enabled: false },
    scrollBeyondLastLine: false,
    wordWrap: "on",
    wrappingIndent: "same",
    renderLineHighlight: "line",
    renderLineHighlightOnlyWhenFocus: true,
    padding: { top: 16, bottom: 16 },
    tabSize: 2,
    insertSpaces: true,
    bracketPairColorization: { enabled: false },
    guides: { indentation: true, bracketPairs: false },
    smoothScrolling: true,
    cursorSmoothCaretAnimation: "on",
    cursorBlinking: "smooth",
    stickyScroll: { enabled: false },
    overviewRulerLanes: 1,
    hideCursorInOverviewRuler: true,
    scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10, useShadows: false },
    fixedOverflowWidgets: true,
    quickSuggestions: { other: true, comments: false, strings: false },
  });
  const { KeyMod, KeyCode } = monaco;
  editor.addCommand(KeyMod.CtrlCmd | KeyCode.KeyS, () => commands.save());
  editor.addCommand(KeyMod.CtrlCmd | KeyCode.KeyO, () => commands.open());
  return editor;
}

export type CodeEditor = monaco.editor.IStandaloneCodeEditor;
export type ViewState = monaco.editor.ICodeEditorViewState | null;

/**
 * One file's text, independent of any editor showing it: a Monaco model plus the operations
 * the app needs (precise edits from the drawing, highlights, diagnostics).
 */
/** Edits kept per document: enough to cover the time between two evaluations. */
const LOG_LIMIT = 2000;

export class TextDoc {
  readonly model: monaco.editor.ITextModel;

  constructor(
    readonly path: string,
    text: string,
    project = "",
  ) {
    const address = [project, ...path.split("/")].filter(Boolean).map(encodeURIComponent).join("/");
    this.model = monaco.editor.createModel(text, "nickel", monaco.Uri.parse(`file:///workspace/${address}`));
    this.logged = this.model.getVersionId();
    this.model.onDidChangeContent((e) => {
      this.log.push({
        from: this.logged,
        changes: e.isFlush
          ? null
          : e.changes.map((c) => ({
              start: c.rangeOffset,
              end: c.rangeOffset + c.rangeLength,
              text: c.text,
            })),
      });
      this.logged = e.versionId;
      if (this.log.length > LOG_LIMIT) this.log.splice(0, this.log.length - LOG_LIMIT);
    });
  }

  get value(): string {
    return this.model.getValue();
  }

  /** Changes with every edit (undo included). */
  get version(): number {
    return this.model.getVersionId();
  }

  /** Recent edits, each with the version it was made on; `null` changes: the text was replaced. */
  private log: { from: number; changes: TextChange[] | null }[] = [];
  private logged: number;

  /**
   * The edits made since `version`, in order, in offsets of the text before each: what turns the
   * text at `version` into the text now. `null` when that's no longer known.
   */
  changesSince(version: number): TextChange[] | null {
    if (version === this.version) return [];
    const first = this.log.findIndex((entry) => entry.from === version);
    if (first < 0) return null;
    const out: TextChange[] = [];
    for (const entry of this.log.slice(first)) {
      if (!entry.changes) return null;
      out.push(...entry.changes);
    }
    return out;
  }

  /** Replace the whole text (keeps undo history). */
  setValue(text: string): void {
    this.model.pushStackElement();
    this.model.pushEditOperations([], [{ range: this.model.getFullModelRange(), text }], () => null);
    this.model.pushStackElement();
  }

  onChange(listener: () => void): monaco.IDisposable {
    return this.model.onDidChangeContent(listener);
  }

  dispose(): void {
    this.model.dispose();
  }

  setDiagnostics(diagnostics: Diagnostic[]): void {
    const severity = {
      error: monaco.MarkerSeverity.Error,
      warning: monaco.MarkerSeverity.Warning,
      info: monaco.MarkerSeverity.Info,
    };
    const lastLine = this.model.getLineCount();
    monaco.editor.setModelMarkers(
      this.model,
      "plate",
      diagnostics.map((d) => {
        const s = d.span ?? { startLine: 1, startCol: 1, endLine: 1, endCol: 1 };
        const startLine = Math.min(s.startLine, lastLine);
        const endLine = Math.min(s.endLine, lastLine);
        // Zero-width spans would be invisible: widen to the word (or the rest of the line).
        let endCol = s.endCol;
        if (startLine === endLine && endCol <= s.startCol) {
          endCol = this.model.getLineMaxColumn(startLine);
        }
        return {
          severity: severity[d.severity],
          message: [d.message, ...(d.notes ?? [])].join("\n"),
          startLineNumber: startLine,
          startColumn: s.startCol,
          endLineNumber: endLine,
          endColumn: endCol,
          source: "plate",
        };
      }),
    );
  }

  // ---------------------------------------------------------------- precise edits
  // Used to write drawing edits (dragging a point) into the exact text that produced a value.

  /** Character offset of a 1-based line/column. */
  offsetOf(line: number, col: number): number {
    return this.model.getOffsetAt({ lineNumber: line, column: col });
  }

  /**
   * Replace text between character offsets. All edits refer to the text before any of them is
   * applied. `undo` controls undo stops, so a whole drag can be a single undo step.
   */
  editOffsets(
    edits: { start: number; end: number; text: string }[],
    undo: { before: boolean; after: boolean } = { before: true, after: true },
  ): void {
    if (undo.before) this.model.pushStackElement();
    this.model.pushEditOperations(
      [],
      edits.map((e) => {
        const a = this.model.getPositionAt(e.start);
        const b = this.model.getPositionAt(e.end);
        return { range: new monaco.Range(a.lineNumber, a.column, b.lineNumber, b.column), text: e.text };
      }),
      () => null,
    );
    if (undo.after) this.model.pushStackElement();
  }

  pushUndoStop(): void {
    this.model.pushStackElement();
  }

  private highlights: string[] = [];

  /** Mark the text being edited from the drawing. */
  highlight(spans: Span[]): void {
    this.highlights = this.model.deltaDecorations(
      this.highlights,
      spans.map((s) => ({
        range: new monaco.Range(s.startLine, s.startCol, s.endLine, s.endCol),
        options: {
          inlineClassName: "live-edit",
          stickiness: monaco.editor.TrackedRangeStickiness.AlwaysGrowsWhenTypingAtEdges,
        },
      })),
    );
  }

  /** The current ranges of the highlighted spans (they follow edits). */
  highlighted(): Span[] {
    return this.highlights
      .map((id) => this.model.getDecorationRange(id))
      .filter((r): r is monaco.Range => !!r)
      .map((r) => ({
        startLine: r.startLineNumber,
        startCol: r.startColumn,
        endLine: r.endLineNumber,
        endCol: r.endColumn,
      }));
  }

  clearHighlight(): void {
    this.highlights = this.model.deltaDecorations(this.highlights, []);
  }
}
