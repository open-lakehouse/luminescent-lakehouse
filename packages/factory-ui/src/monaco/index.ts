// Monaco loads on first use: the canvas and the tree diff don't pay for it.

import { lazy } from "react";

export const MarkdownEditor = lazy(() => import("./MarkdownEditor.js"));
export const TextDiff = lazy(() => import("./TextDiff.js"));
export const NodeDiff = lazy(() => import("./NodeDiff.js"));
export type { MarkdownEditorProps } from "./MarkdownEditor.js";
export type { TextDiffProps } from "./TextDiff.js";
