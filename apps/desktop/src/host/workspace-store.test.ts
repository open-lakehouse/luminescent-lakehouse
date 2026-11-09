import { describe, expect, test } from "bun:test";
import { type Selection, selectionDigest } from "@open-lakehouse/env-editor";
import type { Json } from "./bindings";
import { createFakeState } from "./fake-state";
import { createStateWorkspaceStore, UnsupportedError } from "./workspace-store";

const BASELINE = { modules: ["postgres", "unity-catalog-oss"] } as unknown as Json;

async function setup() {
  const state = createFakeState(BASELINE);
  const created = await state.stateCreateWorkspace("local", false, "dev");
  if (created.status !== "ok") throw new Error("setup");
  const store = createStateWorkspaceStore(created.data.id, state, state.listen);
  return { state, store, workspaceId: created.data.id };
}

const withModules = (modules: string[]) => ({ modules }) as unknown as Selection;

describe("createStateWorkspaceStore", () => {
  test("loads the workspace with its drafts", async () => {
    const { store } = await setup();
    const ws = await store.load();
    expect(ws?.name).toBe("local");
    expect(ws?.envs.map((e) => [e.name, e.slot, e.head])).toEqual([["dev", 0, undefined]]);
    expect(ws?.envs[0]?.selection).toEqual(BASELINE as unknown as Selection);
  });

  test("a new environment keeps the editor's id, and gets the next free slot", async () => {
    const { store, state, workspaceId } = await setup();
    const ws = await store.load();
    if (!ws) throw new Error("no workspace");
    await store.save({
      ...ws,
      envs: [
        ...ws.envs,
        { id: "editor-uuid", name: "staging", slot: 1, selection: withModules([]) },
      ],
    });
    const reloaded = await store.load();
    expect(reloaded?.envs.map((e) => [e.id, e.name, e.slot])).toEqual([
      [ws.envs[0]?.id, "dev", 0],
      ["editor-uuid", "staging", 1],
    ]);
    // Saving through the editor id reaches the store's environment.
    await store.saveEnv({
      id: "editor-uuid",
      name: "staging",
      slot: 1,
      selection: withModules(["x"]),
    });
    const snapshot = await state.stateWorkspace(workspaceId);
    if (snapshot.status !== "ok") throw new Error("snapshot");
    expect(snapshot.data.environments[1]?.selection).toEqual({ modules: ["x"] });
  });

  test("removing an environment deletes it", async () => {
    const { store } = await setup();
    const ws = await store.load();
    if (!ws) throw new Error("no workspace");
    await store.save({ ...ws, envs: [...ws.envs, { id: "s", name: "staging", slot: 1 }] });
    await store.save({ ...ws, envs: ws.envs });
    expect((await store.load())?.envs.map((e) => e.name)).toEqual(["dev"]);
  });

  test("renames and mesh changes are refused before anything is written", async () => {
    const { store } = await setup();
    const ws = await store.load();
    if (!ws) throw new Error("no workspace");
    const dev = ws.envs[0];
    if (!dev) throw new Error("no dev");
    await expect(
      store.save({
        ...ws,
        envs: [
          { ...dev, name: "prod" },
          { id: "n", name: "new", slot: 1 },
        ],
      }),
    ).rejects.toBeInstanceOf(UnsupportedError);
    expect((await store.load())?.envs.map((e) => e.name)).toEqual(["dev"]);
    await expect(store.save({ ...ws, name: "renamed" })).rejects.toThrow("Renaming a workspace");
    await expect(store.save({ ...ws, mesh: true })).rejects.toThrow("gateway mesh");
  });

  test("revisions use the editor's digest, newest first", async () => {
    const { store } = await setup();
    const ws = await store.load();
    const dev = ws?.envs[0];
    if (!dev) throw new Error("no dev");
    const first = await store.commit(dev.id);
    expect(first).toEqual({ number: 1, digest: await selectionDigest(dev.selection as Selection) });
    await expect(store.commit(dev.id)).rejects.toThrow("Nothing to commit");

    await store.saveEnv({ ...dev, selection: withModules(["postgres"]) });
    await store.commit(dev.id, "drop unity-catalog-oss");
    const list = await store.listRevisions(dev.id);
    expect(list.map((r) => r.number)).toEqual([2, 1]);
    expect(list[0]?.digest).toBe(await selectionDigest(withModules(["postgres"])));

    const doc = await store.loadRevision(dev.id, 2);
    expect(doc?.selection).toEqual(withModules(["postgres"]));
    expect(doc?.manifestToml).toContain("[selection]");
    expect(await store.loadRevision(dev.id, 9)).toBeNull();
    expect((await store.load())?.envs[0]?.head).toBe(2);
  });

  test("a write elsewhere (the CLI) notifies subscribers", async () => {
    const { store, state, workspaceId } = await setup();
    let calls = 0;
    const unsubscribe = store.subscribe?.(() => calls++);
    await state.stateCreateEnvironment(workspaceId, "from-cli", null);
    await Bun.sleep(10);
    expect(calls).toBe(1);
    expect((await store.load())?.envs.map((e) => e.name)).toEqual(["dev", "from-cli"]);
    unsubscribe?.();
    await state.stateCreateEnvironment(workspaceId, "later", null);
    await Bun.sleep(10);
    expect(calls).toBe(1);
  });
});
