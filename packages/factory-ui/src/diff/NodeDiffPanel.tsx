// One node in two revisions: its rendered section side by side in Monaco.

import { Suspense } from "react";
import { sectionText } from "../model.js";
import { NodeDiff } from "../monaco/index.js";
import { useRevision } from "../queries.js";

export interface NodeDiffPanelProps {
  root: string;
  base: string;
  target: string;
  nodeKey: string;
}

export function NodeDiffPanel({ root, base, target, nodeKey }: NodeDiffPanelProps) {
  const b = useRevision(root, base || undefined);
  const t = useRevision(root, target);
  const before = base ? sectionText(b.data?.text, b.data?.sections, nodeKey) : undefined;
  const after = sectionText(t.data?.text, t.data?.sections, nodeKey);
  if (!before && !after) {
    return <p className="p-3 text-sm text-muted-foreground">The node isn't in either revision.</p>;
  }
  return (
    <div className="h-full min-h-48" data-testid="node-diff">
      <Suspense
        fallback={<div className="p-3 text-sm text-muted-foreground">Loading the editor…</div>}
      >
        <NodeDiff original={before} modified={after} />
      </Suspense>
    </div>
  );
}
