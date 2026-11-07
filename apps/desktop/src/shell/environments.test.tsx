// The environments in the frame: each one's run state, Start and Stop in its
// tab's header, its sub-tabs on its own services, and the Run panel, on the
// fake state and the real editor.

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { createFakeHost, type FakeHost } from "@open-lakehouse/host/testing";
import {
  cleanup,
  configure,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost, type FakeShellHost } from "../host/fake-host";
import { ShellLayout } from "./ShellLayout";
import type { ServiceRow } from "./shell-host";

// The frame's DOM (the canvas especially) is large: a failed query says what it
// looked for, without printing the document each time `waitFor` retries.
configure({ getElementError: (message) => new Error(message ?? "not found") });

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
});
afterEach(cleanup);

type Harness = FakeShellHost & { host: FakeHost; dev: string; workspaceId: string };

/** The frame on `local/dev` (no revision yet), with `dev`'s tab open. */
export async function renderFrame(
  options: Parameters<typeof createFakeShellHost>[0] = {},
): Promise<Harness> {
  const host = createFakeHost({ kind: "desktop", environment: null });
  const shell = createFakeShellHost({
    onUse: (instance) => host.setEnvironment({ id: instance, name: "dev" }),
    ...options,
  });
  const ws = await shell.workspaces.create("local", "dev");
  const snapshot = await shell.state.stateWorkspace(ws.id);
  if (snapshot.status !== "ok") throw new Error("snapshot");
  const dev = snapshot.data.environments[0]?.id ?? "";
  render(
    <Providers host={host} shell={shell}>
      <ShellLayout />
    </Providers>,
  );
  await waitFor(() => expect(envState()).not.toBeNull());
  return Object.assign(shell, { host, dev, workspaceId: ws.id });
}

/** The open environment's sub-tabs. */
const subTabs = () =>
  document.querySelector<HTMLElement>('[role="tablist"][aria-label="Environment views"]');
/** The open environment's header: state, revision, placement, actions. */
export const header = () => subTabs()?.previousElementSibling as HTMLElement | null | undefined;
/** The open environment's state, as its header's dot says. */
export const envState = () =>
  header()?.querySelector("[data-run-state]")?.getAttribute("data-run-state") ?? null;
const action = (name: string | RegExp) =>
  within(header() as HTMLElement).getByRole("button", { name });
const queryAction = (name: string | RegExp) =>
  within(header() as HTMLElement).queryByRole("button", { name });
const tab = (name: string) => within(subTabs() as HTMLElement).getByRole("tab", { name });
const queryTab = (name: string) => within(subTabs() as HTMLElement).queryByRole("tab", { name });

async function commit(shell: Harness, env = shell.dev) {
  const committed = await shell.state.stateCommit(env);
  if (committed.status !== "ok") throw new Error("commit");
}

/** Click `name` in the header once it is enabled. */
async function clickAction(name: string | RegExp) {
  const button = await waitFor(() => {
    const b = action(name) as HTMLButtonElement;
    expect(b.disabled).toBe(false);
    return b;
  });
  fireEvent.click(button);
}

/** Open the bottom panel's Run view. */
function openRunPanel() {
  fireEvent.click(screen.getByRole("button", { name: /^Run$/ }));
}

test("an environment without a revision can't start, and says why", async () => {
  await renderFrame();
  expect(envState()).toBe("stopped");
  const start = action("Start") as HTMLButtonElement;
  expect(start.disabled).toBe(true);
  expect(start.parentElement?.getAttribute("title")).toBe("Commit a revision first (History)");
});

test("Start runs the head revision and makes it current; Stop takes it down", async () => {
  const shell = await renderFrame();
  await commit(shell);
  await clickAction("Start");
  await waitFor(() => expect(envState()).toBe("running"));
  expect(header()?.textContent).toContain("rev 1");
  expect(header()?.textContent).toContain("local slot 0");
  // What starts becomes the current environment.
  await waitFor(() => expect(shell.host.environment.current()?.id).toBeDefined());

  openRunPanel();
  expect(screen.getByTestId("gateway-url").textContent).toContain("http://localhost:9080");
  expect(screen.getByTestId("run-services").textContent).toContain("unitycatalog");

  fireEvent.click(action("Stop"));
  await waitFor(() => expect(envState()).toBe("stopped"));
  expect(screen.queryByTestId("gateway-url") === null).toBe(true);
});

test("a stopped environment's Catalog says why it can't open", async () => {
  await renderFrame();
  const catalog = tab("Catalog");
  expect(catalog.getAttribute("aria-disabled")).toBe("true");
  expect(catalog.getAttribute("title")).toBe("dev isn't running: Start it");
  expect(tab("Lineage").getAttribute("aria-disabled")).toBe("true");
});

test("opening a running environment's Catalog makes its instance current", async () => {
  const shell = await renderFrame();
  await commit(shell);
  // Started earlier (the CLI, or before a restart): running, but not current.
  const started = await shell.state.runStart(shell.dev);
  if (started.status !== "ok") throw new Error("start");
  await waitFor(() => expect(envState()).toBe("running"));
  expect(shell.host.environment.current()).toBeNull();

  fireEvent.click(tab("Catalog"));
  await waitFor(() => expect(shell.host.environment.current()?.id).toBe(started.data.id));
  // The fake binds nothing: the view says what doesn't serve it, by name.
  await waitFor(() =>
    expect(screen.getByTestId("unbound-unity-catalog").textContent).toContain("dev #1"),
  );
  // A revision without headwaters has no Lineage tab.
  expect(queryTab("Lineage") === null).toBe(true);
});

test("the canvas shows each service's state", async () => {
  const shell = await renderFrame();
  await commit(shell);
  await shell.state.runStart(shell.dev);
  await waitFor(() => expect(envState()).toBe("running"));
  await waitFor(() =>
    expect(
      document.querySelector('.react-flow [data-run-state="running"]') ??
        document.querySelectorAll('[data-run-state="running"]').length > 2,
    ).toBeTruthy(),
  );
});

describe("lifecycle states", () => {
  test("no Docker: the containers are unknown, and Start says why", async () => {
    const shell = await renderFrame();
    await commit(shell);
    shell.state.setEngineProblem("Cannot connect to the Docker daemon");
    await clickAction("Start");
    await waitFor(() => expect(envState()).toBe("failed"));
    openRunPanel();
    await waitFor(() =>
      expect(screen.getAllByRole("alert").map((a) => a.textContent)).toContainEqual(
        expect.stringContaining("Cannot connect to the Docker daemon"),
      ),
    );
  });

  test("starting, healthy, then degraded, each from Docker's events", async () => {
    const shell = await renderFrame();
    await commit(shell);
    const started = await shell.state.runStart(shell.dev);
    if (started.status !== "ok") throw new Error("start");
    const id = started.data.id;
    await waitFor(() => expect(envState()).toBe("running"));

    const all = (over: Partial<ServiceRow>) =>
      started.data.services.map((s) => ({ ...s, ...over }) as ServiceRow);
    shell.state.setServices(id, all({ health: "starting", ready: false }));
    await waitFor(() => expect(envState()).toBe("starting"));
    expect(tab("Catalog").getAttribute("aria-disabled")).toBe("true");

    shell.state.setServices(id, all({ health: "unhealthy", ready: false }));
    await waitFor(() => expect(envState()).toBe("degraded"));

    shell.state.setServices(id, started.data.services);
    await waitFor(() => expect(envState()).toBe("running"));
  });

  test("a port conflict names the port and its owner", async () => {
    const shell = await renderFrame();
    await commit(shell);
    shell.state.failNextStart({ kind: "portsBusy", ports: ["5432 (ssh)"] });
    await clickAction("Start");
    await waitFor(() => expect(envState()).toBe("failed"));
    openRunPanel();
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("5432 (ssh)"));
  });
});

test("one at a time stops the workspace's other environment on start", async () => {
  const shell = await renderFrame();
  await commit(shell);
  const prod = await shell.state.stateCreateEnvironment(shell.workspaceId, "prod", null);
  if (prod.status !== "ok") throw new Error("prod");
  await commit(shell, prod.data.id);
  await shell.state.runStart(shell.dev);
  await waitFor(() => expect(envState()).toBe("running"));

  fireEvent.click(screen.getByRole("button", { name: "Settings" }));
  fireEvent.click(await screen.findByTestId("one-at-a-time"));
  await waitFor(() =>
    expect(screen.getByTestId("one-at-a-time").getAttribute("aria-checked")).toBe("true"),
  );
  fireEvent.keyDown(screen.getByTestId("settings"), { key: "Escape" });
  await waitFor(() => expect(screen.queryByTestId("settings") === null).toBe(true));

  // Open prod from the ⌘K palette and start it.
  fireEvent.keyDown(window, { key: "k", metaKey: true });
  fireEvent.click(await screen.findByText("Go to prod"));
  await waitFor(() => expect(envState()).toBe("stopped"));
  await clickAction("Start");
  await waitFor(() => expect(envState()).toBe("running"));
  const snapshot = await shell.state.runWorkspace(shell.workspaceId);
  if (snapshot.status !== "ok") throw new Error("run");
  const devRun = snapshot.data.environments.find((e) => e.name === "dev");
  expect(devRun?.instances.every((i) => i.desired === "stopped")).toBe(true);
  expect((await shell.run.settings()).oneAtATime).toEqual([shell.workspaceId]);
});

test("Logs follows the instance's containers, filtered by service", async () => {
  const shell = await renderFrame();
  await commit(shell);
  const started = await shell.state.runStart(shell.dev);
  if (started.status !== "ok") throw new Error("start");
  await waitFor(() => expect(tab("Logs").getAttribute("aria-disabled")).toBeNull());
  fireEvent.click(tab("Logs"));
  const lines = await screen.findByTestId("logs-lines");
  await waitFor(() => expect(lines.textContent).toContain("unitycatalog started"));

  shell.state.pushLogs(started.data.id, [
    { service: "postgres", container: "postgres-1", timestamp: null, text: "checkpoint" },
    { service: "rustfs", container: "rustfs-1", timestamp: null, text: "bucket ready" },
  ]);
  await waitFor(() => expect(lines.textContent).toContain("bucket ready"));

  fireEvent.click(screen.getByTestId("logs-filter-postgres"));
  await waitFor(() => expect(lines.textContent).not.toContain("bucket ready"));
  expect(lines.textContent).toContain("checkpoint");

  // Leaving the tab stops the stream.
  fireEvent.click(tab("Design"));
  expect(screen.queryByTestId("logs") === null).toBe(true);
});

test("a running instance's MLflow opens in a window of its own", async () => {
  const opened: [string, string][] = [];
  const shell = await renderFrame({
    serviceUis: () => [
      { moduleId: "mlflow", name: "MLflow", url: "http://local-dev-r1.localhost:9080/mlflow/" },
    ],
    onOpenWindow: (instance, module) => opened.push([instance, module]),
  });
  await commit(shell);
  const started = await shell.state.runStart(shell.dev);
  if (started.status !== "ok") throw new Error("start");
  await clickAction("MLflow");
  await waitFor(() => expect(opened).toEqual([[started.data.id, "mlflow"]]));
});

test("a new revision starts next to the running one, and each stops on its own", async () => {
  const shell = await renderFrame();
  await commit(shell);
  await shell.state.runStart(shell.dev);
  await waitFor(() => expect(envState()).toBe("running"));
  // Only Stop while the head runs.
  expect(queryAction(/^Start/) === null).toBe(true);

  const snapshot = await shell.state.stateWorkspace(shell.workspaceId);
  if (snapshot.status !== "ok") throw new Error("snapshot");
  const selection = snapshot.data.environments[0]?.selection as { modules: string[] };
  await shell.state.stateSetSelection(shell.dev, {
    ...selection,
    modules: [...selection.modules, "headwaters"],
  });
  await commit(shell);

  await clickAction("Start #2");
  await waitFor(() => expect(header()?.textContent).toContain("2 revisions"));
  expect(action("Stop all")).toBeDefined();
  // The revision switch: #2 started last and is current; #1 is a click away.
  expect(header()?.textContent).toContain("rev 2");
  fireEvent.click(action("#1"));
  await waitFor(() => expect(header()?.textContent).toContain("rev 1"));
  expect(header()?.textContent).toContain("local slot 0");

  openRunPanel();
  // Each on its own slot, so its own gateway.
  expect(screen.getAllByTestId("gateway-url").map((g) => g.textContent)).toEqual([
    expect.stringContaining("http://localhost:10080"),
    expect.stringContaining("http://localhost:9080"),
  ]);

  fireEvent.click(action("Stop #1"));
  await waitFor(() => expect(screen.queryByTestId("run-dev-r1") === null).toBe(true));
  expect(screen.getByTestId("run-dev-r2-state").textContent).toBe("running");
  await waitFor(() => expect(action("Stop")).toBeDefined());
  expect(queryAction("Stop all") === null).toBe(true);
});

describe("on Neon", () => {
  /** dev's draft with Unity Catalog and headwaters as Neon Functions, committed. */
  async function onNeon(shell: Harness) {
    const snapshot = await shell.state.stateWorkspace(shell.workspaceId);
    if (snapshot.status !== "ok") throw new Error("snapshot");
    const selection = snapshot.data.environments[0]?.selection as { modules: string[] };
    const fn = { vendor: "neon-function", connection: { kind: "function" } };
    await shell.state.stateSetSelection(shell.dev, {
      ...selection,
      modules: [...new Set([...selection.modules, "unity-catalog", "headwaters"])],
      hosted: { "unity-catalog": fn, headwaters: fn },
    });
    await commit(shell);
  }

  test("the header shows where it runs on Neon, and Start asks only without a target", async () => {
    const started: unknown[] = [];
    const shell = await renderFrame({ onStartOn: (_env, target) => started.push(target) });
    await onNeon(shell);
    // No target yet: the header and the tree say so, before any Start.
    await waitFor(() => expect(header()?.textContent).toContain("☁ no branch yet"));
    expect(action("☁ Pick a Neon branch")).toBeDefined();

    await clickAction("Start");
    const dialog = await screen.findByTestId("neon-target");
    expect(dialog.textContent).toContain("headwaters and unity-catalog as functions");
    fireEvent.click(await screen.findByTestId("neon-target-staging"));
    expect(screen.getByTestId("neon-target-save").textContent).toBe("Save and start");
    fireEvent.click(screen.getByTestId("neon-target-save"));
    await waitFor(() => expect(envState()).toBe("running"));
    expect(started).toEqual([{ project: null, branch: { kind: "existing", branch: "staging" } }]);
    expect(header()?.textContent).toContain("☁ staging · local slot 0");
    // Unity Catalog runs as a function: no container of it, but the tab is there.
    expect(tab("Catalog").getAttribute("aria-disabled")).toBe(null);
    expect(tab("Lineage").getAttribute("aria-disabled")).toBe(null);
    openRunPanel();
    expect(screen.getByTestId("run-services").textContent).not.toContain("unitycatalog");
    // While it runs, the target can't change.
    const picker = action("☁ staging ▾") as HTMLButtonElement;
    expect(picker.disabled).toBe(true);
    expect(picker.parentElement?.getAttribute("title")).toBe(
      "Stop dev to move it to another branch",
    );

    // Stopped: Start runs on the target without asking.
    fireEvent.click(action("Stop"));
    await waitFor(() => expect(envState()).toBe("stopped"));
    expect(header()?.textContent).toContain("☁ staging");
    await clickAction("Start");
    await waitFor(() => expect(envState()).toBe("running"));
    expect(screen.queryByTestId("neon-target") === null).toBe(true);
    expect(started).toHaveLength(2);
  });

  test("the picker sets an environment's own project, or a new branch with a lifetime", async () => {
    const started: unknown[] = [];
    const shell = await renderFrame({ onStartOn: (_env, target) => started.push(target) });
    await onNeon(shell);
    await clickAction("☁ Pick a Neon branch");
    fireEvent.change(await screen.findByTestId("neon-target-project"), {
      target: { value: "quiet-lake-11" },
    });
    fireEvent.click(await screen.findByTestId("neon-target-main"));
    expect(screen.getByTestId("neon-target-save").textContent).toBe("Save");
    fireEvent.click(screen.getByTestId("neon-target-save"));
    await waitFor(() => expect(header()?.textContent).toContain("☁ luminescent-us / main"));
    expect(envState()).toBe("stopped");
    expect((await shell.run.settings()).neonTargets[shell.dev]).toEqual({
      project: "quiet-lake-11",
      branch: { kind: "existing", branch: "main" },
    });

    // Back to the workspace's project, on a new branch for 12 hours.
    fireEvent.click(action("☁ luminescent-us / main ▾"));
    fireEvent.change(await screen.findByTestId("neon-target-project"), {
      target: { value: "\0workspace" },
    });
    fireEvent.click(await screen.findByTestId("neon-target-new"));
    fireEvent.change(screen.getByLabelText("Lifetime"), { target: { value: "12" } });
    fireEvent.click(screen.getByTestId("neon-target-save"));
    await waitFor(() => expect(header()?.textContent).toContain("☁ new branch"));
    await clickAction("Start");
    await waitFor(() => expect(envState()).toBe("running"));
    expect(started).toEqual([{ project: null, branch: { kind: "new", ttlHours: 12 } }]);
    // The branch made for it is its target from now on.
    expect(header()?.textContent).toContain("☁ local-dev-r1");
    expect((await shell.run.settings()).neonTargets[shell.dev]?.branch).toEqual({
      kind: "existing",
      branch: "local-dev-r1",
    });
  });

  test("a local revision has no Neon target, and starts without asking", async () => {
    const asked: unknown[] = [];
    const shell = await renderFrame({ onStartOn: (_env, target) => asked.push(target) });
    await commit(shell);
    expect(queryAction(/Neon/) === null).toBe(true);
    await clickAction("Start");
    await waitFor(() => expect(envState()).toBe("running"));
    expect(screen.queryByTestId("neon-target") === null).toBe(true);
    expect(asked).toEqual([]);
  });
});
