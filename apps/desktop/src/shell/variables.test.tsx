// The Configure tab's Variables and its Models' provider keys, in the frame on the fake
// state and fake vars: each var set's tab, secrets kept in the keychain (the var sets hold
// references), the head revision's vars read-only, an instance's overrides.

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import {
  clickAction,
  clickSelector,
  commit,
  envState,
  type Harness,
  openConfigure,
  renderFrame,
  runFromPalette,
} from "./frame.testing";

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
});
afterEach(cleanup);

async function openVariables() {
  return openConfigure("variables");
}

/** Radix tabs switch on mouse down. */
function openTab(name: string) {
  const trigger = screen.getByTestId(`vars-tab-${name}`);
  fireEvent.mouseDown(trigger, { button: 0 });
  fireEvent.click(trigger);
}

/** Set `key` to `value` with the add row `id` (`vars-local-add`, …). */
function add(id: string, key: string, value: string, secret = false) {
  fireEvent.change(screen.getByTestId(`${id}-key`), { target: { value: key } });
  fireEvent.change(screen.getByTestId(`${id}-value`), { target: { value } });
  if (secret) fireEvent.click(screen.getByTestId(`${id}-secret`));
  fireEvent.click(screen.getByTestId(`${id}-set`));
}

const rows = (id: string) =>
  screen.queryAllByTestId(`${id}-row`).map((r) => ({
    key: r.getAttribute("data-key"),
    secret: r.getAttribute("data-secret") === "true",
    text: r.textContent ?? "",
  }));

describe("Variables", () => {
  test("a literal and a secret on the environment: the secret goes to the keychain", async () => {
    const shell = await renderFrame();
    await openVariables();
    add("vars-environment-add", "REGION", "eu-central-1");
    await waitFor(() => expect(rows("vars-environment").map((r) => r.key)).toEqual(["REGION"]));
    add("vars-environment-add", "API_TOKEN", "t0k3n-value", true);
    await waitFor(() => expect(rows("vars-environment")).toHaveLength(2));
    const token = rows("vars-environment").find((r) => r.key === "API_TOKEN");
    expect(token?.secret).toBe(true);
    expect(token?.text).toContain(`keychain:${shell.dev}/API_TOKEN`);
    expect(token?.text).not.toContain("t0k3n-value");
    expect(shell.vars.keychain.get(`${shell.dev}/API_TOKEN`)).toBe("t0k3n-value");
    // Unset: gone, its keychain entry too.
    const row = screen
      .getAllByTestId("vars-environment-row")
      .find((r) => r.getAttribute("data-key") === "API_TOKEN") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "Unset API_TOKEN" }));
    await waitFor(() => expect(rows("vars-environment")).toHaveLength(1));
    expect(shell.vars.keychain.has(`${shell.dev}/API_TOKEN`)).toBe(false);
  });

  test("each scope has its tab; this machine's says it is never shared", async () => {
    const shell = await renderFrame();
    const dialog = await openVariables();
    openTab("workspace");
    add("vars-workspace-add", "ORG", "acme");
    await waitFor(() => expect(rows("vars-workspace").map((r) => r.key)).toEqual(["ORG"]));
    openTab("local");
    expect(dialog.textContent).toContain("never shared");
    add("vars-local-add", "MINE", "secret-of-mine", true);
    await waitFor(() => expect(rows("vars-local").map((r) => r.key)).toEqual(["MINE"]));
    expect(shell.vars.keychain.get(`${shell.dev}@local/MINE`)).toBe("secret-of-mine");
    // No instance yet: nothing to override.
    expect((screen.getByTestId("vars-tab-instance") as HTMLButtonElement).disabled).toBe(true);
  });

  test("the draft's vars go into the next revision, whose vars are then read-only", async () => {
    const shell = await renderFrame();
    await openVariables();
    openTab("draft");
    expect(screen.getByTestId("configure-variables").textContent).toContain("revision 1");
    add("vars-draft-add", "FEATURE", "on");
    await waitFor(() => expect(rows("vars-draft").map((r) => r.key)).toEqual(["FEATURE"]));
    await commit(shell);
    await waitFor(() => expect(rows("vars-revision").map((r) => r.key)).toEqual(["FEATURE"]));
    expect(screen.queryAllByTestId("vars-revision-unset")).toHaveLength(0);
    expect(screen.getByTestId("configure-variables").textContent).toContain("revision 2");
  });

  test("a running instance's overrides", async () => {
    const shell = await renderFrame();
    await commit(shell);
    await clickAction("Start");
    await waitFor(() => expect(envState()).toBe("running"));
    await openVariables();
    openTab("instance");
    add("vars-instance-add", "LOG_LEVEL", "debug");
    await waitFor(() => expect(rows("vars-instance").map((r) => r.key)).toEqual(["LOG_LEVEL"]));
  });

  test("on a team's store a workspace secret can't be set", async () => {
    const shell = await renderFrame();
    shell.vars.setShared(true);
    const dialog = await openVariables();
    expect(dialog.textContent).toContain("shared with the team");
    openTab("workspace");
    expect((screen.getByTestId("vars-workspace-add-secret") as HTMLButtonElement).disabled).toBe(
      true,
    );
  });

  test("a long value is truncated to its row, and a click wraps it to read", async () => {
    let copied: string[] = [];
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: async (text: string) => void copied.push(text) },
    });
    await renderFrame();
    await openVariables();
    const long = `postgres://${"x".repeat(300)}@example.com/db`;
    add("vars-environment-add", "DATABASE_URL", long);
    const value = await waitFor(() => {
      const [v] = screen.getAllByTestId("vars-environment-value");
      if (!v) throw new Error("no value yet");
      return v;
    });
    expect(value.className).toContain("truncate");
    expect(value.getAttribute("title")).toBe(long);
    fireEvent.click(value);
    expect(value.getAttribute("data-wrapped")).toBe("true");
    expect(value.className).toContain("break-all");
    fireEvent.click(screen.getByTestId("vars-environment-value-copy"));
    await waitFor(() => expect(copied).toEqual([long]));

    // A secret's is never offered for copying.
    copied = [];
    add("vars-environment-add", "API_TOKEN", "t0k3n-value", true);
    await waitFor(() => expect(rows("vars-environment")).toHaveLength(2));
    const token = screen
      .getAllByTestId("vars-environment-row")
      .find((r) => r.getAttribute("data-key") === "API_TOKEN") as HTMLElement;
    expect(within(token).queryByTestId("vars-environment-value-copy")).toBeNull();
  });

  test("⌘K opens it", async () => {
    await renderFrame();
    await runFromPalette("Edit dev's variables…");
    await screen.findByTestId("configure-variables");
  });
});

describe("Models' provider keys", () => {
  async function withMlflow(shell: Harness) {
    const snapshot = await shell.state.stateWorkspace(shell.workspaceId);
    if (snapshot.status !== "ok") throw new Error("snapshot");
    const selection = snapshot.data.environments[0]?.selection as { modules: string[] };
    await shell.state.stateSetSelection(shell.dev, {
      ...selection,
      modules: [...new Set([...selection.modules, "mlflow"])],
    });
    await commit(shell);
  }

  test("a key set there is a workspace secret, and no longer missing", async () => {
    const shell = await renderFrame();
    await withMlflow(shell);
    await clickSelector("Models");
    await screen.findByTestId("configure-models");
    expect(screen.getByTestId("models-no-keys")).toBeDefined();
    const id = "models-key-ANTHROPIC_API_KEY";
    fireEvent.change(await screen.findByTestId(`${id}-value`), {
      target: { value: "sk-ant-fake" },
    });
    fireEvent.click(screen.getByTestId(`${id}-set`));
    await waitFor(() =>
      expect(screen.getByTestId(`${id}-where`).textContent).toBe("Set on the workspace"),
    );
    expect(shell.vars.keychain.get(`${shell.workspaceId}/ANTHROPIC_API_KEY`)).toBe("sk-ant-fake");
    await waitFor(() => expect(screen.queryByTestId("models-no-keys") === null).toBe(true));
    // The key isn't the choice: nothing is saved yet.
    expect(shell.models.choices.get(shell.dev)).toBeUndefined();
  });
});
