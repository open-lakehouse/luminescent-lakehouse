// What the review's build puts in place of factory-ui's Monaco module
// (vite.config.ts): the view offers the tree diff only, and Monaco's workers
// can't be inlined into one document.

import type { ComponentType } from "react";

function Unavailable() {
  return <p className="p-3 text-sm text-muted-foreground">Not available in the plan review.</p>;
}

export const MarkdownEditor: ComponentType<unknown> = Unavailable;
export const TextDiff: ComponentType<unknown> = Unavailable;
export const NodeDiff: ComponentType<unknown> = Unavailable;
