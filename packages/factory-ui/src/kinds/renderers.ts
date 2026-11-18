// Rendering payloads: a KindRenderer registry keyed by type URL for the
// kinds and conditions this client knows, and a generic fallback for
// everything else, decoded from GetSchema's descriptors. Nothing here names
// an extension's types: they render generically, labelled by the display
// names their extension registers in the schema.

import { type JsonValue, type Message, type Registry, toJson } from "@bufbuild/protobuf";
import { type Any, anyUnpack, timestampDate } from "@bufbuild/protobuf/wkt";
import {
  CONDITION,
  type Decision,
  type Gate,
  type HumanGate,
  KIND,
  type Node,
  type NotBefore,
  type Plan,
  type PrMerged,
  type Schema,
  shortKind,
  type Task,
} from "@open-lakehouse/factory-client";

/** What a renderer can look things up in. */
export interface RenderContext {
  schema?: Schema;
  registry: Registry;
  /** A node's title by ref, for conditions that name a node. */
  titleOf?: (ref: string) => string | undefined;
}

/** How this client shows one payload type it knows. */
export interface KindRenderer {
  /** One line for a card or a chip; `undefined` to fall back to the
   * type's display name. */
  summary(message: Message, ctx: RenderContext): string | undefined;
}

const renderers = new Map<string, KindRenderer>();

/** Register (or replace) the renderer for a payload type. */
export function registerKindRenderer(typeUrl: string, renderer: KindRenderer) {
  renderers.set(typeUrl, renderer);
}

export function kindRenderer(typeUrl: string): KindRenderer | undefined {
  return renderers.get(typeUrl);
}

/** A payload decoded with `registry`, or `undefined` when its type isn't
 * in it. */
export function decodeAny(any: Any | undefined, registry: Registry): Message | undefined {
  if (!any?.typeUrl) return undefined;
  try {
    return anyUnpack(any, registry);
  } catch {
    return undefined;
  }
}

/** A decoded payload as proto-JSON, for the generic view. */
export function payloadJson(message: Message, registry: Registry): JsonValue | undefined {
  const desc = registry.getMessage(message.$typeName);
  if (!desc) return undefined;
  try {
    return toJson(desc, message, { registry });
  } catch {
    return undefined;
  }
}

/** Where a payload sits: a node's own, a gate's condition, or a task's
 * extension. One type may be registered in more than one role. */
export type PayloadRole = "kind" | "condition" | "extension";

/** The display name the schema gives a type URL in `role` (any role when
 * unset); else its short name. */
export function payloadLabel(
  typeUrl: string,
  schema: Schema | undefined,
  role?: PayloadRole,
): string {
  const kind = () => schema?.nodeKinds.find((k) => k.kind === typeUrl)?.displayName;
  const condition = () => schema?.gateConditions.find((k) => k.typeUrl === typeUrl)?.displayName;
  const extension = () => schema?.taskExtensions.find((k) => k.typeUrl === typeUrl)?.displayName;
  const named =
    role === "kind"
      ? kind()
      : role === "condition"
        ? condition()
        : role === "extension"
          ? extension()
          : kind() || condition() || extension();
  return named || shortKind(typeUrl);
}

/** One line for a payload: its renderer's summary, else its label. */
export function summarize(
  any: Any | undefined,
  ctx: RenderContext,
  role?: PayloadRole,
): string | undefined {
  if (!any?.typeUrl) return undefined;
  const label = payloadLabel(any.typeUrl, ctx.schema, role);
  const renderer = kindRenderer(any.typeUrl);
  const message = decodeAny(any, ctx.registry);
  const summary = renderer && message ? renderer.summary(message, ctx) : undefined;
  return summary ?? label;
}

/** The chips a node card shows beyond its kind: a gate's condition, and a
 * task's extensions. */
export function cardChips(node: Node, ctx: RenderContext): string[] {
  const payload = node.spec?.payload;
  const message = decodeAny(payload, ctx.registry);
  if (!message) return [];
  if (node.kind === KIND.gate) {
    const s = summarize((message as Gate).condition, ctx, "condition");
    return s ? [s] : [];
  }
  if (node.kind === KIND.task) {
    return (message as Task).extensions.flatMap((e) => {
      const s = summarize(e, ctx, "extension");
      return s ? [s] : [];
    });
  }
  return [];
}

// The reference kinds and conditions.
registerKindRenderer(KIND.plan, { summary: (m) => (m as Plan).goal || undefined });
registerKindRenderer(KIND.task, { summary: (m) => (m as Task).repo || undefined });
registerKindRenderer(KIND.decision, {
  summary: (m) => {
    const d = m as Decision;
    return d.outcome ? `Decided: ${d.outcome}` : "Undecided";
  },
});
registerKindRenderer(KIND.gate, {
  summary: (m, ctx) => summarize((m as Gate).condition, ctx, "condition") ?? "No condition yet",
});
registerKindRenderer(CONDITION.prMerged, {
  summary: (m, ctx) => {
    const task = (m as PrMerged).task;
    return `Merged: ${ctx.titleOf?.(task) ?? task}`;
  },
});
registerKindRenderer(CONDITION.human, {
  summary: (m) => {
    const prompt = (m as HumanGate).prompt.split("\n")[0];
    return prompt ? `Person: ${prompt}` : "Person";
  },
});
registerKindRenderer(CONDITION.notBefore, {
  summary: (m) => {
    const at = (m as NotBefore).at;
    return at ? `Not before ${timestampDate(at).toISOString().slice(0, 10)}` : "Not before";
  },
});
