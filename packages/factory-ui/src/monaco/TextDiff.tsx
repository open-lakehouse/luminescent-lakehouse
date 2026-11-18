// Two revisions' rendered plans side by side in Monaco's diff editor.
// Selecting a node elsewhere reveals its section here; moving the cursor into
// a section here selects its node.

import { DiffEditor, type DiffOnMount } from "@monaco-editor/react";
import { ensureMonacoSetup, useMonacoTheme } from "@open-lakehouse/editor";
import type * as Monaco from "monaco-editor";
import { useEffect, useRef } from "react";
import { sectionAt } from "../model.js";

ensureMonacoSetup();

type Sections = { key: string; startLine: number; endLine: number }[];

export interface TextDiffProps {
  original: string;
  modified: string;
  originalSections?: Sections;
  modifiedSections?: Sections;
  /** The node whose section to reveal. */
  selected?: string;
  onSelect?: (key: string) => void;
}

export default function TextDiff({
  original,
  modified,
  originalSections,
  modifiedSections,
  selected,
  onSelect,
}: TextDiffProps) {
  const theme = useMonacoTheme();
  const editor = useRef<Monaco.editor.IStandaloneDiffEditor | null>(null);
  const onSelectRef = useRef(onSelect);
  onSelectRef.current = onSelect;
  const sectionsRef = useRef({ originalSections, modifiedSections });
  sectionsRef.current = { originalSections, modifiedSections };

  const onMount: DiffOnMount = (e) => {
    editor.current = e;
    e.getModifiedEditor().onDidChangeCursorPosition((ev) => {
      if (ev.source !== "mouse" && ev.source !== "keyboard") return;
      const key = sectionAt(sectionsRef.current.modifiedSections, ev.position.lineNumber);
      if (key) onSelectRef.current?.(key);
    });
    e.getOriginalEditor().onDidChangeCursorPosition((ev) => {
      if (ev.source !== "mouse") return;
      const key = sectionAt(sectionsRef.current.originalSections, ev.position.lineNumber);
      if (key) onSelectRef.current?.(key);
    });
  };

  useEffect(() => {
    const e = editor.current;
    if (!e || !selected) return;
    const inModified = modifiedSections?.find((s) => s.key === selected);
    const inOriginal = originalSections?.find((s) => s.key === selected);
    if (inModified) {
      e.getModifiedEditor().revealLinesInCenter(inModified.startLine, inModified.endLine);
      e.getModifiedEditor().setSelection({
        startLineNumber: inModified.startLine,
        startColumn: 1,
        endLineNumber: inModified.startLine,
        endColumn: 1,
      });
    } else if (inOriginal) {
      e.getOriginalEditor().revealLinesInCenter(inOriginal.startLine, inOriginal.endLine);
    }
  }, [selected, modifiedSections, originalSections]);

  // Let the widget release its models before they go (see mangrove's DiffHost).
  useEffect(
    () => () => {
      const e = editor.current;
      const models = e?.getModel();
      e?.setModel(null);
      models?.original.dispose();
      models?.modified.dispose();
    },
    [],
  );

  return (
    <div className="h-full" data-testid="text-diff">
      <DiffEditor
        original={original}
        modified={modified}
        language="markdown"
        theme={theme}
        onMount={onMount}
        keepCurrentOriginalModel
        keepCurrentModifiedModel
        options={{
          readOnly: true,
          originalEditable: false,
          renderSideBySide: true,
          useInlineViewWhenSpaceIsLimited: true,
          minimap: { enabled: false },
          wordWrap: "on",
          scrollBeyondLastLine: false,
          fontSize: 13,
          hideUnchangedRegions: { enabled: false },
        }}
      />
    </div>
  );
}
