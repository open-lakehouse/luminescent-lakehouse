// One node's section in two revisions, in mangrove's read-only Monaco diff.

import { DiffHost } from "@open-lakehouse/editor";

export default function NodeDiff({ original, modified }: { original?: string; modified?: string }) {
  return <DiffHost path="node.md" original={original} modified={modified} />;
}
