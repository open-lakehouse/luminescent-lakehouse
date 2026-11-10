// A rendered file of an environment in Monaco (mangrove's `ReadOnlyFile`):
// read-only, its language from the extension, in the app's light or dark
// theme. env-editor's Files tab shows it in place of its own viewer
// (`fileViewer`); index.tsx loads it lazily.

import { ReadOnlyFile } from "@open-lakehouse/editor";
import type { FileViewerProps } from "@open-lakehouse/env-editor";
import { useEffect, useState } from "react";

type Contents =
  | { state: "loading" }
  | { state: "error"; message: string }
  | { state: "ready"; text: string };

export function FileViewer({ path, read }: FileViewerProps) {
  const [contents, setContents] = useState<Contents>({ state: "loading" });
  useEffect(() => {
    let live = true;
    setContents({ state: "loading" });
    Promise.resolve()
      .then(() => read(path))
      .then(
        (text) => live && setContents({ state: "ready", text }),
        (e: unknown) =>
          live &&
          setContents({ state: "error", message: e instanceof Error ? e.message : String(e) }),
      );
    return () => {
      live = false;
    };
  }, [path, read]);

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-testid="monaco-file">
      <div className="truncate border-b px-3 py-2 font-mono text-[11px] text-muted-foreground">
        {path}
      </div>
      {contents.state === "ready" ? (
        <ReadOnlyFile path={path} value={contents.text} className="min-h-0 flex-1" />
      ) : (
        <div
          className={
            contents.state === "error"
              ? "p-6 text-xs text-destructive"
              : "p-6 text-xs text-muted-foreground"
          }
          role={contents.state === "error" ? "alert" : undefined}
        >
          {contents.state === "error" ? contents.message : `Loading ${path}…`}
        </div>
      )}
    </div>
  );
}
