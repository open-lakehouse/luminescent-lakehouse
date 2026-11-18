// The reference kinds and edge kinds, packing payloads into `Any`, and the
// registry that unpacks the payloads a server's extensions register.

import {
  create,
  createFileRegistry,
  createRegistry,
  type DescFile,
  type DescMessage,
  fromBinary,
  type MessageInitShape,
  type MessageShape,
  type Registry,
} from "@bufbuild/protobuf";
import {
  type Any,
  anyPack,
  anyUnpack,
  type FileDescriptorSet,
  FileDescriptorSetSchema,
} from "@bufbuild/protobuf/wkt";
import { file_factory_workgraph_v1_execution } from "./gen/factory/workgraph/v1/execution_pb.js";
import { file_factory_workgraph_v1_kinds } from "./gen/factory/workgraph/v1/kinds_pb.js";
import { file_factory_workgraph_v1_records } from "./gen/factory/workgraph/v1/records_pb.js";

const TYPE_URL_PREFIX = "type.googleapis.com/";

/** The type URL of a message type, as nodes carry it in `kind`. */
export function typeUrl(schema: DescMessage): string {
  return TYPE_URL_PREFIX + schema.typeName;
}

/** The reference node kinds, annotation kinds and step payload kinds. */
export const KIND = {
  plan: `${TYPE_URL_PREFIX}factory.workgraph.v1.Plan`,
  task: `${TYPE_URL_PREFIX}factory.workgraph.v1.Task`,
  decision: `${TYPE_URL_PREFIX}factory.workgraph.v1.Decision`,
  gate: `${TYPE_URL_PREFIX}factory.workgraph.v1.Gate`,
  learning: `${TYPE_URL_PREFIX}factory.workgraph.v1.Learning`,
  reconcileNote: `${TYPE_URL_PREFIX}factory.workgraph.v1.ReconcileNote`,
  decisionOutcome: `${TYPE_URL_PREFIX}factory.workgraph.v1.DecisionOutcome`,
  omnigentSession: `${TYPE_URL_PREFIX}factory.workgraph.v1.OmnigentSession`,
} as const;

/** The reference gate conditions. */
export const CONDITION = {
  prMerged: `${TYPE_URL_PREFIX}factory.workgraph.v1.PrMerged`,
  human: `${TYPE_URL_PREFIX}factory.workgraph.v1.HumanGate`,
  notBefore: `${TYPE_URL_PREFIX}factory.workgraph.v1.NotBefore`,
} as const;

/** The built-in edge kinds. */
export const EDGE = {
  childOf: "child_of",
  blockedBy: "blocked_by",
  relatesTo: "relates_to",
  discoveredFrom: "discovered_from",
} as const;

/** The generated files of the payload types this client has code for. */
export const builtinFiles: readonly DescFile[] = [
  file_factory_workgraph_v1_kinds,
  file_factory_workgraph_v1_records,
  file_factory_workgraph_v1_execution,
];

/** The payload types this client can unpack on its own: node kinds,
 * annotations, and the typed payloads of run steps. */
export const payloadRegistry = createRegistry(...builtinFiles);

/** The registry for a server: every type in its GetSchema descriptors
 * (`fileDescriptorSet`, an encoded `google.protobuf.FileDescriptorSet`),
 * with this client's generated types on top. Payloads of an extension this
 * client has no code for unpack as generic messages. Falls back to
 * {@link payloadRegistry} when the set is empty or doesn't decode. */
export function schemaRegistry(fileDescriptorSet: Uint8Array | undefined): Registry {
  if (!fileDescriptorSet?.length) return payloadRegistry;
  try {
    const server = createFileRegistry(fromBinary(FileDescriptorSetSchema, fileDescriptorSet));
    return createRegistry(server, ...builtinFiles);
  } catch {
    return payloadRegistry;
  }
}

/** The files and their imports as a `FileDescriptorSet`, imports first: what
 * a server's GetSchema returns for them. */
export function fileDescriptorSet(files: readonly DescFile[]): FileDescriptorSet {
  const seen = new Set<string>();
  const out: DescFile[] = [];
  const visit = (f: DescFile) => {
    if (seen.has(f.name)) return;
    seen.add(f.name);
    for (const d of f.dependencies) visit(d);
    out.push(f);
  };
  for (const f of files) visit(f);
  return create(FileDescriptorSetSchema, { file: out.map((f) => f.proto) });
}

/** `Task` for `type.googleapis.com/factory.workgraph.v1.Task`. */
export function shortKind(kind: string): string {
  return kind.split(/[/.]/).pop() ?? kind;
}

/** Unpack a payload whose type `registry` knows; `undefined` otherwise. */
export function unpackPayload(payload: Any | undefined, registry: Registry = payloadRegistry) {
  if (!payload) return undefined;
  try {
    return anyUnpack(payload, registry);
  } catch {
    return undefined;
  }
}

/** Pack a payload message under its own type URL. */
export function packPayload<Desc extends DescMessage>(
  schema: Desc,
  value: MessageInitShape<Desc> | MessageShape<Desc>,
): Any {
  return anyPack(schema, create(schema, value as MessageInitShape<Desc>));
}
