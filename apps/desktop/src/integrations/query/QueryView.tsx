// The Query app: SQL against one running instance's Unity Catalog, in the host's
// query engine (src-tauri/src/query.rs). mangrove's Monaco editor above
// arrow-grid's `DataGrid`. Cmd/Ctrl+Enter runs the selection, or all of it. The
// text stays per instance for as long as the app runs. Completion comes from the
// instance's catalog, through its binding.

import { DataGrid } from "@open-lakehouse/arrow-grid";
import {
  ensureModel,
  getEntry,
  MonacoHost,
  type MonacoHostProps,
  registerCatalogProvider,
} from "@open-lakehouse/editor";
import { Button } from "@open-lakehouse/ui-kit";
import { ucCatalogProvider } from "@open-lakehouse/unity-catalog-ui";
import { Play, Square } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { useShellHost } from "../../shell/shell-host";
import { QueryStatus } from "./QueryStatus";
import { useQueryRun } from "./useQueryRun";

registerCatalogProvider(ucCatalogProvider);

// Monaco's types, as the editor hands them over (monaco-editor is its dependency).
type Mount = NonNullable<MonacoHostProps["onEditorMount"]>;
type Editor = Parameters<Mount>[1];

/** What a new instance's editor starts with. */
export const STARTER_SQL = `-- Cmd/Ctrl+Enter runs the selection, or all of it.
-- Tables are catalog.schema.table, as Unity Catalog names them.
SELECT 1 AS one`;

/** The model's path for `instance`: one buffer per instance. */
export const queryPath = (instance: string) => `/query/${instance}.sql`;

/** The text of each instance's editor, for as long as the app runs: the editor
 *  disposes its model when it unmounts (another app, another instance). */
const drafts = new Map<string, string>();

export function QueryView({ instance, label }: { instance: string; label: string }) {
  const shell = useShellHost();
  const runner = useMemo(() => shell.query.runner(instance), [shell, instance]);
  const result = useQueryRun(runner);
  const path = queryPath(instance);
  const [activeId, setActiveId] = useState<string | null>(null);
  const editorRef = useRef<Editor | null>(null);

  const attach = useCallback<Mount>(
    (monaco, editor) => {
      editorRef.current = editor;
      const { model } = ensureModel(monaco, path, drafts.get(path) ?? STARTER_SQL);
      model.onDidChangeContent(() => drafts.set(path, model.getValue()));
      setActiveId(path);
    },
    [path],
  );

  const { run } = result;
  const runActive = useCallback(() => {
    const model = getEntry(path)?.model;
    if (!model) return;
    const selection = editorRef.current?.getSelection();
    const selected = selection && !selection.isEmpty() ? model.getValueInRange(selection) : "";
    const sql = (selected || model.getValue()).trim();
    if (sql) void run(sql);
  }, [path, run]);

  return (
    <div className="flex h-full min-h-0 flex-col" data-testid="query-view">
      <div className="flex h-9 shrink-0 items-center gap-2 border-b px-3">
        <span className="truncate text-xs text-muted-foreground">SQL on {label}</span>
        <div className="ml-auto flex items-center gap-2">
          <QueryStatus
            running={result.running}
            rows={result.rows}
            elapsedMs={result.elapsedMs}
            failed={result.error !== null}
          />
          {result.running ? (
            <Button size="sm" variant="outline" onClick={result.cancel}>
              <Square className="size-3.5" />
              Cancel
            </Button>
          ) : (
            <Button size="sm" onClick={runActive} disabled={activeId === null}>
              <Play className="size-3.5" />
              Run
            </Button>
          )}
        </div>
      </div>
      <div className="min-h-0 flex-1 basis-1/2 border-b">
        <MonacoHost activeId={activeId} onRun={runActive} onEditorMount={attach} />
      </div>
      <div className="flex min-h-0 flex-1 basis-1/2 flex-col" data-testid="query-results">
        {result.error ? (
          <p className="whitespace-pre-wrap p-3 text-sm text-destructive" role="alert">
            {result.error}
          </p>
        ) : result.version === 0 ? (
          <p className="p-3 text-sm text-muted-foreground">Run a query to see its rows here.</p>
        ) : (
          <DataGrid store={result.store} version={result.version} running={result.running} />
        )}
      </div>
    </div>
  );
}
