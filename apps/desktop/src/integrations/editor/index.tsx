// The shell's Monaco viewers for env-editor's Files and Changes
// (`fileViewer`, `diffViewer`), loaded on first use: Monaco and its workers
// stay out of the app's first chunk.

import type { DiffViewerProps, FileViewerProps } from "@open-lakehouse/env-editor";
import { lazy, type ReactNode, Suspense } from "react";

const LazyFileViewer = lazy(() => import("./FileViewer").then((m) => ({ default: m.FileViewer })));
const LazyDiffViewer = lazy(() => import("./DiffViewer").then((m) => ({ default: m.DiffViewer })));

function Loading({ children }: { children: ReactNode }) {
  return <div className="p-6 text-xs text-muted-foreground">{children}</div>;
}

export function fileViewer(props: FileViewerProps): ReactNode {
  return (
    <Suspense key={props.path} fallback={<Loading>Loading the editor…</Loading>}>
      <LazyFileViewer {...props} />
    </Suspense>
  );
}

export function diffViewer(props: DiffViewerProps): ReactNode {
  return (
    <Suspense key={props.path} fallback={<Loading>Loading the diff…</Loading>}>
      <LazyDiffViewer {...props} />
    </Suspense>
  );
}
