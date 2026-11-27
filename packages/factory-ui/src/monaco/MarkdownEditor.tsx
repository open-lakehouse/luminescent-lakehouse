// A small Monaco editor for a node's markdown fields, on the same Monaco
// setup and theme bridge as mangrove's @open-lakehouse/editor.

import Editor from "@monaco-editor/react";
import { ensureMonacoSetup, useMonacoTheme } from "@open-lakehouse/editor";

ensureMonacoSetup();

export interface MarkdownEditorProps {
  value: string;
  onChange: (value: string) => void;
  /** Pixels. */
  height?: number;
  language?: "markdown" | "json" | "plaintext";
  readOnly?: boolean;
  ariaLabel?: string;
  /** Focus the editor once it mounts (it replaces a field being edited). */
  autoFocus?: boolean;
}

export default function MarkdownEditor({
  value,
  onChange,
  height = 160,
  language = "markdown",
  readOnly,
  ariaLabel,
  autoFocus,
}: MarkdownEditorProps) {
  const theme = useMonacoTheme();
  return (
    <section
      className="overflow-hidden rounded-md border"
      style={{ height }}
      aria-label={ariaLabel}
    >
      <Editor
        value={value}
        language={language}
        theme={theme}
        onChange={(v) => onChange(v ?? "")}
        onMount={(editor) => {
          if (autoFocus) editor.focus();
        }}
        options={{
          readOnly,
          minimap: { enabled: false },
          lineNumbers: "off",
          wordWrap: "on",
          scrollBeyondLastLine: false,
          fontSize: 13,
          folding: false,
          renderLineHighlight: "none",
          padding: { top: 6, bottom: 6 },
        }}
      />
    </section>
  );
}
