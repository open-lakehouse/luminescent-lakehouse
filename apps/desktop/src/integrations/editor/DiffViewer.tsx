// One file's change in Monaco's diff editor (mangrove's `DiffHost`): read-only,
// side by side (inline when narrow), unchanged regions folded. env-editor's
// Changes list shows one per file (`diffViewer`), stacked, so each takes a
// height from its longer side, capped; index.tsx loads it lazily.

import { DiffHost } from "@open-lakehouse/editor";
import type { DiffViewerProps } from "@open-lakehouse/env-editor";
import { diffHeight } from "./diffHeight";

export function DiffViewer({ path, original, modified }: DiffViewerProps) {
  return (
    <figure
      className="m-0 overflow-hidden rounded-md border"
      style={{ height: diffHeight(original, modified) }}
      aria-label={`Changes to ${path}`}
      data-testid="monaco-diff"
    >
      <DiffHost path={path} original={original} modified={modified} />
    </figure>
  );
}
