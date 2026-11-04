// The environment editor's `WorkspaceStore` on trestle's shared state store, the
// one the `trestle` CLI uses (shell-core's `state`, over IPC).
//
// What the editor and the store model differently, and how this adapter bridges it:
// - Ids: the editor mints an id for a new environment before the store does. The
//   adapter maps editor ids to store ids for the session, so open tabs survive the
//   reload that follows every write.
// - Slots: the store leases a slot when an instance starts (#18); the editor only
//   uses one to preview host ports. The adapter assigns the lowest free slot to each
//   environment it sees and keeps it for the session.
// - Digests: the store's digest covers manifest, modules and vars; the editor
//   compares a revision with its draft by `selectionDigest`. Revisions are reported
//   with the editor's digest, so "same as #N" works.
// - Not in the store yet (open-lakehouse/trestle#313): renaming a workspace or an
//   environment, changing the gateway mesh, and a commit's message and time. Those
//   are refused with a clear error, and the message is dropped.

import {
  type EnvDoc,
  type RevisionDoc,
  type RevisionStore,
  type RevisionSummary,
  type Selection,
  selectionDigest,
  type WorkspaceDoc,
} from "@open-lakehouse/env-editor";
import {
  commands,
  type EnvironmentDraft_Serialize as EnvironmentDraft,
  events,
  type RevisionInfo,
  type StateError,
  type WorkspaceSnapshot_Serialize as WorkspaceSnapshot,
} from "./bindings";

type Result<T, E> = { status: "ok"; data: T } | { status: "error"; error: E };

/** The commands the adapter uses (all of `commands` in the app; a fake in tests). */
export type StateApi = Pick<
  typeof commands,
  | "stateWorkspace"
  | "stateCreateEnvironment"
  | "stateDeleteEnvironment"
  | "stateSetSelection"
  | "stateRevisions"
  | "stateRevision"
  | "stateCommit"
>;

/** Subscribe to `state-changed`; resolves to the unlisten function. */
export type StateEvents = (onChange: () => void) => Promise<() => void>;

export const listenStateChanged: StateEvents = (onChange) =>
  events.stateChanged.listen(() => onChange());

/** `listen`, as the synchronous subscribe the editor and the views expect. */
export function subscribeWith<T = void>(
  listen: (onChange: (value: T) => void) => Promise<() => void>,
  onChange: (value: T) => void,
): () => void {
  let unlisten: (() => void) | undefined;
  let cancelled = false;
  void listen(onChange).then((fn) => {
    if (cancelled) fn();
    else unlisten = fn;
  });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}

/** Something the store can't do yet: shown in the editor as is. */
export class UnsupportedError extends Error {
  constructor(what: string) {
    super(`${what} isn't supported by trestle's shared state yet.`);
    this.name = "UnsupportedError";
  }
}

/** What to show for `error`. */
export function stateErrorMessage(error: StateError): string {
  return error.kind === "noChanges"
    ? `Nothing to commit: the draft is the same as revision #${error.head}.`
    : error.message;
}

function unwrap<T>(result: Result<T, StateError>): T {
  if (result.status === "error") throw new Error(stateErrorMessage(result.error));
  return result.data;
}

async function summary(revision: RevisionInfo): Promise<RevisionSummary> {
  return {
    number: revision.number,
    digest: await selectionDigest(revision.selection as Selection),
  };
}

export function createStateWorkspaceStore(
  workspaceId: string,
  api: StateApi = commands,
  listen: StateEvents = listenStateChanged,
): RevisionStore {
  const toStore = new Map<string, string>();
  const toEditor = new Map<string, string>();
  const slots = new Map<string, number>();
  let last: WorkspaceSnapshot | null = null;

  const storeId = (editorId: string) => toStore.get(editorId) ?? editorId;
  const editorId = (id: string) => toEditor.get(id) ?? id;

  const slotOf = (id: string): number => {
    const known = slots.get(id);
    if (known !== undefined) return known;
    const taken = new Set(slots.values());
    let slot = 0;
    while (taken.has(slot)) slot++;
    slots.set(id, slot);
    return slot;
  };

  const toEnvDoc = (env: EnvironmentDraft): EnvDoc => ({
    id: editorId(env.id),
    name: env.name,
    slot: slotOf(env.id),
    selection: env.selection as Selection,
    ...(env.head != null ? { head: env.head } : {}),
  });

  async function snapshot(): Promise<WorkspaceSnapshot> {
    last = unwrap(await api.stateWorkspace(workspaceId));
    return last;
  }

  return {
    async load() {
      const ws = await snapshot();
      return { name: ws.name, mesh: ws.mesh, envs: ws.environments.map(toEnvDoc) };
    },

    async save(next: WorkspaceDoc) {
      const current = last ?? (await snapshot());
      if (next.name !== current.name) throw new UnsupportedError("Renaming a workspace");
      if (next.mesh !== current.mesh) throw new UnsupportedError("Changing the gateway mesh");
      const existing = new Map(current.environments.map((e) => [e.id, e]));
      // Check everything before writing anything, so a refused save changes nothing.
      for (const env of next.envs) {
        const stored = existing.get(storeId(env.id));
        if (stored && stored.name !== env.name) {
          throw new UnsupportedError("Renaming an environment");
        }
      }
      const kept = new Set(next.envs.map((e) => storeId(e.id)));
      for (const id of existing.keys()) {
        if (!kept.has(id)) unwrap(await api.stateDeleteEnvironment(id));
      }
      for (const env of next.envs) {
        if (existing.has(storeId(env.id))) continue;
        const created = unwrap(
          await api.stateCreateEnvironment(workspaceId, env.name, env.selection ?? null),
        );
        toStore.set(env.id, created.id);
        toEditor.set(created.id, env.id);
        slots.set(created.id, env.slot);
      }
      await snapshot();
    },

    async saveEnv(env: EnvDoc) {
      if (!env.selection) return;
      unwrap(await api.stateSetSelection(storeId(env.id), env.selection));
    },

    subscribe: (onChange) => subscribeWith(listen, onChange),

    async listRevisions(envId) {
      const revisions = unwrap(await api.stateRevisions(storeId(envId)));
      return Promise.all(revisions.map(summary));
    },

    async loadRevision(envId, number): Promise<RevisionDoc | null> {
      const revision = unwrap(await api.stateRevision(storeId(envId), number));
      if (!revision) return null;
      return {
        ...(await summary(revision)),
        selection: revision.selection as Selection,
        manifestToml: revision.manifestToml,
      };
    },

    // The store keeps no commit message yet; it is dropped.
    async commit(envId) {
      return summary(unwrap(await api.stateCommit(storeId(envId))));
    },
  };
}
