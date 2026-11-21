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
// - Versions: each draft carries the version it was read at (`draftVersion`), and a
//   save is refused with a conflict when the draft was edited since (by the CLI,
//   another window, a colleague on a shared store), never overwriting that edit. The
//   editor reloads on every `state-changed`, so the version taken from the newest
//   load is the one of the draft it shows. Saves of one environment run one at a
//   time, each on the version the previous one returned.
// - Write errors: the editor shows a store's errors only while loading, so the
//   adapter also reports a failed write to `onWriteError`, with a reload for a
//   conflict.
// - Not wired up yet: renaming a workspace and changing the gateway mesh (the store
//   can, trestle#313 aside; see docs/desktop.md). Those are refused with a clear error.

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
  type RevisionInfo_Serialize as RevisionInfo,
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
  | "stateRenameEnvironment"
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

/** The draft was edited elsewhere since the editor loaded it: nothing was written. */
export class ConflictError extends Error {
  constructor(envName: string) {
    super(
      `${envName} was edited elsewhere, so your change wasn't saved. Reload to see the change, then edit again.`,
    );
    this.name = "ConflictError";
  }
}

/** What the adapter reports besides throwing (the editor shows errors only on load). */
export interface StateStoreOptions {
  /** A write failed; `reload` loads the workspace again (what a conflict needs). */
  onWriteError?: (error: Error, reload: () => void) => void;
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
    createdAt: revision.createdAt,
    ...(revision.message ? { message: revision.message } : {}),
  };
}

export function createStateWorkspaceStore(
  workspaceId: string,
  api: StateApi = commands,
  listen: StateEvents = listenStateChanged,
  options: StateStoreOptions = {},
): RevisionStore {
  const toStore = new Map<string, string>();
  const toEditor = new Map<string, string>();
  const slots = new Map<string, number>();
  // Store id -> the draft version the editor's copy is at. Versions only grow, so
  // the newer of a load's and a save's is the current one.
  const versions = new Map<string, number>();
  // Store id -> the environment's last save: the next one waits for it.
  const saving = new Map<string, Promise<unknown>>();
  // Who to tell to load again (the editor subscribes), besides `state-changed`.
  const reloaders = new Set<() => void>();
  let last: WorkspaceSnapshot | null = null;

  const known = (id: string, version: number) =>
    versions.set(id, Math.max(version, versions.get(id) ?? version));
  const reload = () => {
    for (const r of [...reloaders]) r();
  };
  /** Report a failed write to `onWriteError` too, then reject with it. */
  const reported = async <T>(write: () => Promise<T>): Promise<T> => {
    try {
      return await write();
    } catch (e) {
      const error = e instanceof Error ? e : new Error(String(e));
      options.onWriteError?.(error, reload);
      throw error;
    }
  };

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
    for (const env of last.environments) known(env.id, env.draftVersion);
    return last;
  }

  return {
    async load() {
      const ws = await snapshot();
      return { name: ws.name, mesh: ws.mesh, envs: ws.environments.map(toEnvDoc) };
    },

    save: (next: WorkspaceDoc) =>
      reported(async () => {
        const current = last ?? (await snapshot());
        if (next.name !== current.name) throw new UnsupportedError("Renaming a workspace");
        if (next.mesh !== current.mesh) throw new UnsupportedError("Changing the gateway mesh");
        const existing = new Map(current.environments.map((e) => [e.id, e]));
        const kept = new Set(next.envs.map((e) => storeId(e.id)));
        for (const id of existing.keys()) {
          if (!kept.has(id)) unwrap(await api.stateDeleteEnvironment(id));
        }
        for (const env of next.envs) {
          const stored = existing.get(storeId(env.id));
          if (stored) {
            if (stored.name !== env.name) {
              unwrap(await api.stateRenameEnvironment(stored.id, env.name));
            }
            continue;
          }
          const created = unwrap(
            await api.stateCreateEnvironment(workspaceId, env.name, env.selection ?? null),
          );
          toStore.set(env.id, created.id);
          toEditor.set(created.id, env.id);
          slots.set(created.id, env.slot);
          known(created.id, created.draftVersion);
        }
        await snapshot();
      }),

    saveEnv(env: EnvDoc) {
      const selection = env.selection;
      if (!selection) return Promise.resolve();
      const id = storeId(env.id);
      const write = async () => {
        const expected = versions.get(id);
        if (expected === undefined) throw new Error(`${env.name} isn't loaded yet.`);
        const result = await api.stateSetSelection(id, selection, expected);
        if (result.status === "error" && result.error.kind === "conflict") {
          throw new ConflictError(env.name);
        }
        known(id, unwrap(result));
      };
      // After the environment's previous save, whatever became of it.
      const next = (saving.get(id) ?? Promise.resolve()).catch(() => {}).then(write);
      saving.set(id, next);
      return reported(() => next);
    },

    subscribe: (onChange) => {
      reloaders.add(onChange);
      const unlisten = subscribeWith(listen, onChange);
      return () => {
        reloaders.delete(onChange);
        unlisten();
      };
    },

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

    async commit(envId, message) {
      return summary(unwrap(await api.stateCommit(storeId(envId), message?.trim() || null)));
    },
  };
}
