// Edit one node: its title and body, its kind's payload, and its phase; see
// its derived conditions, its latest run (steps, sessions, PRs) and its
// annotations. What has no editor renders generically from GetSchema's
// descriptors: a gate's condition, a task's extensions, a kind this client
// has no code for. Fields the form doesn't edit (a Task's repo, profiles,
// extensions) are kept as they are on save. Saves are optimistic updates on the
// node's etag: if someone else changed it first, the save is refused and the
// form reloads.

import { clone, create, isMessage, type Registry } from "@bufbuild/protobuf";
import { Code, ConnectError } from "@connectrpc/connect";
import {
  DecisionSchema,
  GateSchema,
  KIND,
  type Node,
  NodeSpecSchema,
  type OmnigentSession,
  Phase,
  PlanSchema,
  packPayload,
  payloadRegistry,
  type Run,
  type Schema,
  TaskSchema,
  unpackPayload,
} from "@open-lakehouse/factory-client";
import {
  Badge,
  Button,
  Input,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Textarea,
} from "@open-lakehouse/ui-kit";
import { Trash2 } from "lucide-react";
import { Suspense, useEffect, useMemo, useState } from "react";
import { GenericPayload, summarize } from "../kinds/index.js";
import {
  displayName,
  PHASE_LABEL,
  PLACEMENT_SOURCE_LABEL,
  prLabel,
  pullRequests,
  sessions,
  stageLabel,
  usd,
} from "../model.js";
import { MarkdownEditor } from "../monaco/index.js";
import { useAnnotations, useGraphMutations } from "../queries.js";

export interface NodeInspectorProps {
  root: string;
  node: Node;
  /** The node's latest run, if it has one. */
  run?: Run;
  schema: Schema | undefined;
  /** Decodes payloads from the server's descriptors (`usePayloadRegistry`);
   * the built-in types by default. */
  registry?: Registry;
  readOnly?: boolean;
  onDeleted?: () => void;
  /** Opens one of the run's Omnigent sessions in the host (the desktop's
   * Agent view); without it, a session links to its `uiUrl`, if it has one. */
  onOpenSession?: (session: OmnigentSession) => void;
}

/** The editable fields of a node, as a flat draft. */
export interface Draft {
  title: string;
  body: string;
  /** Task. */
  acceptance: string;
  touchedPaths: string;
  /** Plan. */
  goal: string;
  rubric: string;
  criteria: string;
  /** Decision. */
  question: string;
  options: string;
  outcome: string;
}

const lines = (s: string) =>
  s
    .split("\n")
    .map((l) => l.trim())
    .filter(Boolean);

export function draftOf(node: Node): Draft {
  const p = unpackPayload(node.spec?.payload) as Record<string, unknown> | undefined;
  const str = (k: string) => (typeof p?.[k] === "string" ? (p[k] as string) : "");
  const list = (k: string) => (Array.isArray(p?.[k]) ? (p[k] as string[]).join("\n") : "");
  return {
    title: node.spec?.title ?? "",
    body: node.spec?.body ?? "",
    acceptance: str("acceptance"),
    touchedPaths: list("touchedPaths"),
    goal: str("goal"),
    rubric: list("rubric"),
    criteria: list("criteria"),
    question: str("question"),
    options: list("options"),
    outcome: str("outcome"),
  };
}

/** The payload to save: the draft over the node's current payload. Exported
 * for tests. */
export function payloadOf(kind: string, d: Draft, node: Node) {
  // The fields the form edits, over the payload as it is, so the rest
  // (repo, profiles, extensions, …) survive a save.
  const current = unpackPayload(node.spec?.payload);
  switch (kind) {
    case KIND.task: {
      const t = isMessage(current, TaskSchema) ? clone(TaskSchema, current) : create(TaskSchema);
      t.acceptance = d.acceptance;
      t.touchedPaths = lines(d.touchedPaths);
      return packPayload(TaskSchema, t);
    }
    case KIND.plan: {
      const p = isMessage(current, PlanSchema) ? clone(PlanSchema, current) : create(PlanSchema);
      p.goal = d.goal;
      p.rubric = lines(d.rubric);
      p.criteria = lines(d.criteria);
      return packPayload(PlanSchema, p);
    }
    case KIND.decision: {
      const x = isMessage(current, DecisionSchema)
        ? clone(DecisionSchema, current)
        : create(DecisionSchema);
      x.question = d.question;
      x.options = lines(d.options);
      x.outcome = d.outcome;
      return packPayload(DecisionSchema, x);
    }
    default:
      // A kind this client can't decode: its payload is kept as it is.
      return node.spec?.payload;
  }
}

function Field({
  label,
  htmlFor,
  children,
}: {
  label: string;
  htmlFor?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <Label htmlFor={htmlFor} className="text-xs text-muted-foreground">
        {label}
      </Label>
      {children}
    </div>
  );
}

function Markdown(props: {
  value: string;
  onChange: (v: string) => void;
  label: string;
  readOnly?: boolean;
  height?: number;
}) {
  return (
    <Suspense
      fallback={<Textarea value={props.value} readOnly rows={4} aria-label={props.label} />}
    >
      <MarkdownEditor
        value={props.value}
        onChange={props.onChange}
        readOnly={props.readOnly}
        ariaLabel={props.label}
        height={props.height}
      />
    </Suspense>
  );
}

export function NodeInspector({
  root,
  node,
  run,
  schema,
  registry = payloadRegistry,
  readOnly,
  onDeleted,
  onOpenSession,
}: NodeInspectorProps) {
  const m = useGraphMutations(root);
  const annotations = useAnnotations(root, node.ref);
  const [draft, setDraft] = useState<Draft>(() => draftOf(node));
  const [notice, setNotice] = useState<string>();
  // A new node, or a new version of it from the server, resets the form.
  // biome-ignore lint/correctness/useExhaustiveDependencies: keyed on identity and etag.
  useEffect(() => {
    setDraft(draftOf(node));
  }, [node.ref, node.metadata?.etag]);

  const original = useMemo(() => draftOf(node), [node]);
  const dirty = JSON.stringify(original) !== JSON.stringify(draft);
  const set = (k: keyof Draft) => (v: string) => setDraft((d) => ({ ...d, [k]: v }));
  const known = [KIND.task, KIND.plan, KIND.decision].includes(node.kind as (typeof KIND)["task"]);
  const isRoot = node.ref === root;
  const current = unpackPayload(node.spec?.payload, registry);
  const condition = isMessage(current, GateSchema) ? current.condition : undefined;
  // Extensions are ordered and may repeat a type, so position is the key.
  const keyedExtensions = (isMessage(current, TaskSchema) ? current.extensions : []).map(
    (extension, n) => ({ key: `${n}:${extension.typeUrl}`, extension }),
  );

  const save = async () => {
    setNotice(undefined);
    try {
      await m.updateNode.mutateAsync({
        ref: node.ref,
        etag: node.metadata?.etag ?? "",
        spec: create(NodeSpecSchema, {
          title: draft.title,
          body: draft.body,
          payload: payloadOf(node.kind, draft, node),
        }),
      });
    } catch (e) {
      const err = ConnectError.from(e);
      setNotice(
        err.code === Code.Aborted
          ? "Someone changed this node since you opened it; the form shows their version."
          : err.rawMessage,
      );
      if (err.code === Code.Aborted) setDraft(draftOf(node));
    }
  };

  const conditions = node.status?.conditions ?? [];
  const phase = node.status?.phase ?? Phase.UNSPECIFIED;

  return (
    <form
      className="flex h-full min-h-0 flex-col gap-3 overflow-auto p-3"
      aria-label={`Edit ${node.spec?.title}`}
      onSubmit={(e) => {
        e.preventDefault();
        void save();
      }}
    >
      <div className="flex items-center gap-2">
        <Badge variant="outline">{displayName(schema, node.kind)}</Badge>
        <code className="truncate text-[11px] text-muted-foreground" title={node.ref}>
          {node.ref}
        </code>
        {node.metadata?.backend ? <Badge variant="default">{node.metadata.backend}</Badge> : null}
      </div>

      <Field label="Title" htmlFor="node-title">
        <Input
          id="node-title"
          value={draft.title}
          onChange={(e) => set("title")(e.target.value)}
          readOnly={readOnly}
        />
      </Field>

      <Field label="Phase">
        <Select
          value={String(phase)}
          disabled={readOnly}
          onValueChange={(v) => m.setPhase.mutate({ ref: node.ref, phase: Number(v) as Phase })}
        >
          <SelectTrigger aria-label="Phase">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {[Phase.PROPOSED, Phase.APPROVED, Phase.DONE, Phase.DROPPED].map((p) => (
              <SelectItem key={p} value={String(p)}>
                {PHASE_LABEL[p]}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </Field>

      {conditions.length ? (
        <ul className="flex flex-col gap-1 text-xs" aria-label="Conditions">
          {conditions.map((c) => (
            <li key={c.type} className="flex gap-2">
              <span className={c.status ? "font-medium" : "text-muted-foreground"}>
                {c.type}: {c.status ? "yes" : "no"}
              </span>
              <span className="text-muted-foreground">{c.message || c.reason}</span>
            </li>
          ))}
        </ul>
      ) : null}

      {node.kind === KIND.task ? (
        <>
          <Field label="Acceptance">
            <Markdown
              value={draft.acceptance}
              onChange={set("acceptance")}
              label="Acceptance"
              readOnly={readOnly}
            />
          </Field>
          <Field label="Touched paths (one per line)" htmlFor="node-paths">
            <Textarea
              id="node-paths"
              rows={3}
              value={draft.touchedPaths}
              onChange={(e) => set("touchedPaths")(e.target.value)}
              readOnly={readOnly}
              className="font-mono text-xs"
            />
          </Field>
        </>
      ) : null}
      {node.kind === KIND.plan ? (
        <>
          <Field label="Goal" htmlFor="node-goal">
            <Input
              id="node-goal"
              value={draft.goal}
              onChange={(e) => set("goal")(e.target.value)}
              readOnly={readOnly}
            />
          </Field>
          <Field label="Rubric (one per line)" htmlFor="node-rubric">
            <Textarea
              id="node-rubric"
              rows={3}
              value={draft.rubric}
              onChange={(e) => set("rubric")(e.target.value)}
              readOnly={readOnly}
            />
          </Field>
          <Field label="Criteria links (one per line)" htmlFor="node-criteria">
            <Textarea
              id="node-criteria"
              rows={2}
              value={draft.criteria}
              onChange={(e) => set("criteria")(e.target.value)}
              readOnly={readOnly}
            />
          </Field>
        </>
      ) : null}
      {node.kind === KIND.decision ? (
        <>
          <Field label="Question" htmlFor="node-question">
            <Input
              id="node-question"
              value={draft.question}
              onChange={(e) => set("question")(e.target.value)}
              readOnly={readOnly}
            />
          </Field>
          <Field label="Options (one per line)" htmlFor="node-options">
            <Textarea
              id="node-options"
              rows={3}
              value={draft.options}
              onChange={(e) => set("options")(e.target.value)}
              readOnly={readOnly}
            />
          </Field>
          <Field label="Outcome">
            <Select
              value={draft.outcome || "__none"}
              disabled={readOnly}
              onValueChange={(v) => set("outcome")(v === "__none" ? "" : v)}
            >
              <SelectTrigger aria-label="Outcome">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="__none">Undecided</SelectItem>
                {lines(draft.options).map((o) => (
                  <SelectItem key={o} value={o}>
                    {o}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </Field>
        </>
      ) : null}
      {node.kind === KIND.gate ? (
        <Field label="Condition">
          {condition?.typeUrl ? (
            <>
              <span className="text-xs" data-testid="gate-condition">
                {summarize(condition, { schema, registry }, "condition")}
              </span>
              <GenericPayload
                payload={condition}
                registry={registry}
                schema={schema}
                place="condition"
              />
            </>
          ) : (
            <span className="text-xs text-muted-foreground">No condition yet.</span>
          )}
        </Field>
      ) : null}
      {keyedExtensions.length ? (
        <Field label="Extensions">
          <section className="flex flex-col gap-1" aria-label="Extensions">
            {keyedExtensions.map(({ key, extension }) => (
              <GenericPayload
                key={key}
                payload={extension}
                registry={registry}
                schema={schema}
                place="extension"
              />
            ))}
          </section>
        </Field>
      ) : null}
      {!known && node.kind !== KIND.gate ? (
        <div className="flex flex-col gap-1" data-testid="unknown-payload">
          {node.spec?.payload?.typeUrl ? (
            <GenericPayload
              payload={node.spec.payload}
              registry={registry}
              schema={schema}
              place="kind"
            />
          ) : null}
          <p className="text-xs text-muted-foreground">
            This client has no editor for <code>{node.kind}</code>; its payload is kept as it is
            when you save.
          </p>
        </div>
      ) : null}

      <Field label="Notes (markdown)">
        <Markdown
          value={draft.body}
          onChange={set("body")}
          label="Notes"
          readOnly={readOnly}
          height={120}
        />
      </Field>

      {notice ? (
        <p role="alert" className="rounded-md bg-destructive/10 p-2 text-xs text-destructive">
          {notice}
        </p>
      ) : null}

      {!readOnly ? (
        <div className="flex gap-2">
          <Button type="submit" size="sm" disabled={!dirty || m.updateNode.isPending}>
            Save
          </Button>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            disabled={!dirty}
            onClick={() => setDraft(original)}
          >
            Discard
          </Button>
          {!isRoot ? (
            <Button
              type="button"
              size="sm"
              variant="ghost"
              className="ml-auto text-destructive"
              onClick={async () => {
                if (!window.confirm(`Delete "${node.spec?.title}" and everything below it?`))
                  return;
                await m.deleteNode.mutateAsync({ ref: node.ref, recursive: true });
                onDeleted?.();
              }}
            >
              <Trash2 /> Delete
            </Button>
          ) : null}
        </div>
      ) : null}

      {run ? <RunSection run={run} onOpenSession={onOpenSession} /> : null}

      <section aria-label="Annotations" className="flex flex-col gap-2 border-t pt-3">
        <h3 className="text-xs font-medium text-muted-foreground">Annotations</h3>
        {(annotations.data ?? []).length === 0 ? (
          <p className="text-xs text-muted-foreground">None yet.</p>
        ) : null}
        {(annotations.data ?? []).map((a) => {
          const learning =
            a.kind === KIND.learning
              ? (unpackPayload(a.payload) as { lesson?: string; evidence?: string[] } | undefined)
              : undefined;
          return (
            <article key={a.ref} className="rounded-md border p-2 text-xs">
              <div className="mb-1 flex gap-2 text-muted-foreground">
                <span>{a.kind.split(".").pop()}</span>
                <span>by {a.provenance?.actor}</span>
              </div>
              {learning ? (
                <>
                  <p>{learning.lesson}</p>
                  {learning.evidence?.length ? (
                    <ul className="mt-1 list-disc pl-4 text-muted-foreground">
                      {learning.evidence.map((e) => (
                        <li key={e}>{e}</li>
                      ))}
                    </ul>
                  ) : null}
                </>
              ) : null}
            </article>
          );
        })}
      </section>
    </form>
  );
}

/** The node's latest run: stage, cost, each session step, and its PRs. */
function RunSection({
  run,
  onOpenSession,
}: {
  run: Run;
  onOpenSession?: (session: OmnigentSession) => void;
}) {
  const prs = pullRequests(run);
  return (
    <section aria-label="Run" className="flex flex-col gap-2 border-t pt-3 text-xs">
      <h3 className="flex gap-2 font-medium text-muted-foreground">
        <span>
          Run {run.attempt}
          {run.executor ? ` (${run.executor})` : ""}
        </span>
        <span className="text-foreground">{stageLabel(run.stage)}</span>
        {run.costUsd ? <span className="ml-auto">{usd(run.costUsd)}</span> : null}
      </h3>
      <ol className="flex flex-col gap-1">
        {sessions(run).map(({ step, name, session }) => (
          <li key={step} className="rounded-md border p-2">
            <div className="flex gap-2">
              <span className="font-medium">{name || `step ${step}`}</span>
              <span className="text-muted-foreground">{session.status}</span>
              {onOpenSession && session.sessionId ? (
                <button
                  type="button"
                  className="ml-auto underline"
                  aria-label={`Open session ${name || `step ${step}`}`}
                  onClick={() => onOpenSession(session)}
                >
                  open
                </button>
              ) : session.uiUrl ? (
                <a
                  href={session.uiUrl}
                  target="_blank"
                  rel="noreferrer"
                  className="ml-auto underline"
                >
                  open
                </a>
              ) : null}
            </div>
            <div className="text-muted-foreground">
              {[
                session.spec?.agentName,
                session.spec?.agentVersion && `v${session.spec.agentVersion}`,
                session.spec?.harness,
                session.spec?.model,
                session.spec?.environment &&
                  `in ${session.spec.environment} (${PLACEMENT_SOURCE_LABEL[session.spec.placementSource]})`,
              ]
                .filter(Boolean)
                .join(" · ")}
            </div>
            <code className="text-[11px] text-muted-foreground">{session.sessionId}</code>
          </li>
        ))}
      </ol>
      {prs.length ? (
        <ul className="flex flex-col gap-1" aria-label="Pull requests">
          {prs.map((pr) => (
            <li key={pr.url} className="flex gap-2">
              <a href={pr.url} target="_blank" rel="noreferrer" className="underline">
                {prLabel(pr)}
              </a>
              <span className="truncate" title={pr.title}>
                {pr.title}
              </span>
              <span className="ml-auto text-muted-foreground">
                {[pr.state, pr.checks, pr.reviewDecision].filter(Boolean).join(" · ")}
              </span>
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}
