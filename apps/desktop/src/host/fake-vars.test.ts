import { describe, expect, test } from "bun:test";
import { createFakeState } from "./fake-state";
import { createFakeVars, MASK } from "./fake-vars";

async function setup() {
  const state = createFakeState({ modules: [] });
  const ws = await state.stateCreateWorkspace("local", false, "dev");
  if (ws.status === "error") throw new Error("no workspace");
  const snapshot = await state.stateWorkspace(ws.data.id);
  if (snapshot.status === "error") throw new Error("no snapshot");
  const env = snapshot.data.environments[0]?.id ?? "";
  return { state, vars: createFakeVars(state), ws: ws.data.id, env };
}

describe("fake vars", () => {
  test("a secret goes to the keychain under the CLI's key; the var set holds its reference", async () => {
    const { vars, ws, env } = await setup();
    await vars.set({ scope: "workspace", workspace: ws }, "KEY", {
      kind: "secret",
      value: "s3cret",
    });
    await vars.set({ scope: "local", environment: env }, "KEY", { kind: "secret", value: "mine" });
    expect(vars.keychain.get(`${ws}/KEY`)).toBe("s3cret");
    expect(vars.keychain.get(`${env}@local/KEY`)).toBe("mine");
    const view = await vars.get(env, null);
    expect(view.workspace).toEqual([{ key: "KEY", secret: true, value: `keychain:${ws}/KEY` }]);
    expect(JSON.stringify(view)).not.toContain("s3cret");
    await vars.unset({ scope: "local", environment: env }, "KEY");
    expect(vars.keychain.has(`${env}@local/KEY`)).toBe(false);
  });

  test("on a team's store an environment secret stays on this machine", async () => {
    const { vars, ws, env } = await setup();
    vars.setShared(true);
    const went = await vars.set({ scope: "environment", environment: env }, "K", {
      kind: "secret",
      value: "v",
    });
    expect(went).toEqual({ scope: "local", environment: env });
    await expect(
      vars.set({ scope: "workspace", workspace: ws }, "K", { kind: "secret", value: "v" }),
    ).rejects.toThrow(/shared with the team/);
  });

  test("the effective view layers the var sets, the branch and the overrides", async () => {
    const { state, vars, ws, env } = await setup();
    await vars.set({ scope: "workspace", workspace: ws }, "A", { kind: "literal", value: "ws" });
    await vars.set({ scope: "draft", environment: env }, "A", { kind: "literal", value: "rev" });
    await vars.set({ scope: "local", environment: env }, "S", { kind: "secret", value: "x" });
    await state.stateCommit(env);
    const started = await state.runStart(env, false, { kind: "existing", branch: "main" });
    if (started.status === "error") throw new Error("no start");
    const id = started.data.id;
    await vars.set({ scope: "instance", instance: id }, "A", { kind: "literal", value: "mine" });
    const view = await vars.effective(id);
    const a = view.entries.find((e) => e.key === "A");
    expect(a?.value).toBe("mine");
    expect(a?.layer.name).toBe("instance overrides");
    expect(a?.shadowed.map((l) => l.name)).toEqual(["workspace vars", "revision vars"]);
    const s = view.entries.find((e) => e.key === "S");
    expect(s).toMatchObject({ value: MASK, secret: true, reference: `keychain:${env}@local/S` });
    expect(view.entries.find((e) => e.key === "PGHOST")?.layer.label).toBe("Neon branch main");
    expect(view.dotenv).toContain("# Neon branch main\nDATABASE_URL=••••••\n");
    expect(view.dotenv).not.toContain("=x\n");
  });
});
