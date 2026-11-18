// Any payload as a property list, decoded from GetSchema's descriptors: how
// the control room shows a kind, a gate condition or a task extension it has
// no code for.

import type { JsonValue, Registry } from "@bufbuild/protobuf";
import type { Any } from "@bufbuild/protobuf/wkt";
import type { Schema } from "@open-lakehouse/factory-client";
import { decodeAny, type PayloadRole, payloadJson, payloadLabel } from "./renderers.js";

export interface GenericPayloadProps {
  payload: Any;
  registry: Registry;
  schema?: Schema;
  /** Where it sits, to pick its display name. */
  place?: PayloadRole;
  /** Shown above the fields; the type's display name by default. */
  label?: string;
}

function Value({ value }: { value: JsonValue }) {
  if (value === null) return <span className="text-muted-foreground">null</span>;
  if (Array.isArray(value)) {
    return (
      <ul className="list-disc pl-4">
        {value.map((v, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: a static list.
          <li key={i}>
            <Value value={v} />
          </li>
        ))}
      </ul>
    );
  }
  if (typeof value === "object") return <Properties object={value} />;
  return <span className="break-words">{String(value)}</span>;
}

function Properties({ object }: { object: { [k: string]: JsonValue } }) {
  const entries = Object.entries(object).filter(([k]) => k !== "@type");
  if (!entries.length) return <span className="text-muted-foreground">(empty)</span>;
  return (
    <dl className="grid grid-cols-[auto_1fr] gap-x-2 gap-y-0.5">
      {entries.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="text-muted-foreground">{k}</dt>
          <dd className="min-w-0">
            <Value value={v} />
          </dd>
        </div>
      ))}
    </dl>
  );
}

export function GenericPayload({ payload, registry, schema, place, label }: GenericPayloadProps) {
  const title = label ?? payloadLabel(payload.typeUrl, schema, place);
  const message = decodeAny(payload, registry);
  const json = message ? payloadJson(message, registry) : undefined;
  return (
    <section
      className="rounded-md border p-2 text-xs"
      aria-label={title}
      data-testid="generic-payload"
      data-type-url={payload.typeUrl}
    >
      <h4 className="mb-1 font-medium" title={payload.typeUrl}>
        {title}
      </h4>
      {json !== undefined && json !== null && typeof json === "object" && !Array.isArray(json) ? (
        <Properties object={json} />
      ) : (
        <p className="text-muted-foreground" data-testid="undecodable-payload">
          No descriptor for <code>{payload.typeUrl}</code> ({payload.value.length} bytes); it's kept
          as it is.
        </p>
      )}
    </section>
  );
}
