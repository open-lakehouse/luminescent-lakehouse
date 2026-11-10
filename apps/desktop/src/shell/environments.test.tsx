// The environments in the frame: each one's run state, Start and Stop and
// details in its tab's header, and the bottom panel's Run and Logs, on the fake state and the real
// editor. Their apps are apps.test.tsx's.

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import {
  action,
  appButton,
  appsActivity,
  clickAction,
  commit,
  envState,
  type Harness,
  header,
  headerDetail,
  openLogsPanel,
  openRunPanel,
  queryAction,
  queryTab,
  renderFrame,
  runFromPalette,
} from "./frame.testing";
import type { ServiceRow } from "./shell-host";

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
});
afterEach(cleanup);

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

  // The gateway's URL is a header detail, with a copy button.
  expect(headerDetail("Gateway")).toBe("http://localhost:9080");
  expect(
    within(header() as HTMLElement).getByRole("button", { name: "Copy Gateway" }),
  ).toBeDefined();

  openRunPanel();
  expect(screen.getByTestId("gateway-url").textContent).toContain("http://localhost:9080");
  expect(screen.getByTestId("run-services").textContent).toContain("unitycatalog");

  fireEvent.click(action("Stop"));
  await waitFor(() => expect(envState()).toBe("stopped"));
  expect(screen.queryByTestId("gateway-url") === null).toBe(true);
  expect(headerDetail("Gateway")).toBeUndefined();
});

test("the Run panel's gateway opens the instance's apps", async () => {
  const shell = await renderFrame();
  await commit(shell);
  await shell.state.runStart(shell.dev);
  await waitFor(() => expect(envState()).toBe("running"));
  openRunPanel();
  fireEvent.click(await screen.findByTestId("gateway-url"));
  await waitFor(() => expect(appButton("Catalog")).toBeDefined());
});

test("⌘K runs an environment's Start and Stop, each with its CLI equivalent", async () => {
  const shell = await renderFrame();
  await commit(shell);
  fireEvent.keyDown(window, { key: "k", metaKey: true, ctrlKey: true });
  // The hint is the CLI's equivalent.
  await waitFor(() => expect(screen.getByText("trestle up local/dev@1")).toBeDefined());
  fireEvent.click(screen.getByText("Start dev"));
  await waitFor(() => expect(envState()).toBe("running"));
  await runFromPalette("Stop dev");
  await waitFor(() => expect(envState()).toBe("stopped"));
});

test("⌘K copies an environment's CLI command", async () => {
  const shell = await renderFrame();
  await commit(shell);
  let copied: string | undefined;
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: {
      writeText: async (text: string) => {
        copied = text;
      },
    },
  });
  await runFromPalette("Copy CLI: trestle up local/dev@1");
  await waitFor(() => expect(copied).toBe("trestle up local/dev@1"));
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
    // Its apps wait until it is up.
    expect((action("Apps") as HTMLButtonElement).disabled).toBe(true);

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
  // Not a sub-tab any more: the bottom panel's, next to Run.
  expect(queryTab("Logs") === null).toBe(true);
  await waitFor(() => expect(envState()).toBe("running"));
  openLogsPanel();
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
  openRunPanel();
  expect(screen.queryByTestId("logs") === null).toBe(true);
});

test("Logs says when there is nothing to follow, and ⌘K opens it", async () => {
  const shell = await renderFrame();
  openLogsPanel();
  expect(screen.getByRole("region", { name: "Panel" }).textContent).toContain(
    "dev has no containers to follow",
  );
  openRunPanel();
  await commit(shell);
  await shell.state.runStart(shell.dev);
  await waitFor(() => expect(envState()).toBe("running"));
  await runFromPalette("Show logs of dev");
  expect(await screen.findByTestId("logs-lines")).toBeDefined();
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
  /** dev's draft with Unity Catalog and headwaters as Neon Functions, committed
   *  unless `draft`. */
  async function onNeon(shell: Harness, { draft = false } = {}) {
    const snapshot = await shell.state.stateWorkspace(shell.workspaceId);
    if (snapshot.status !== "ok") throw new Error("snapshot");
    const selection = snapshot.data.environments[0]?.selection as { modules: string[] };
    const fn = { vendor: "neon-function", connection: { kind: "function" } };
    await shell.state.stateSetSelection(shell.dev, {
      ...selection,
      modules: [...new Set([...selection.modules, "unity-catalog-rs", "headwaters"])],
      hosted: { "unity-catalog-rs": fn, headwaters: fn },
    });
    if (!draft) await commit(shell);
  }

  test("a draft that runs something on Neon can pick its branch before a revision needs one", async () => {
    const shell = await renderFrame();
    await onNeon(shell, { draft: true });
    await waitFor(() => expect(headerDetail("Neon")).toBe("no branch yet"));
    await clickAction("☁ Pick a Neon branch");
    fireEvent.click(await screen.findByTestId("neon-target-staging"));
    expect(screen.getByTestId("neon-target-save").textContent).toBe("Save");
    fireEvent.click(screen.getByTestId("neon-target-save"));
    await waitFor(() => expect(headerDetail("Neon")).toBe("staging"));
    expect((await shell.run.settings()).neonTargets[shell.dev]).toEqual({
      project: null,
      branch: { kind: "existing", branch: "staging" },
    });
  });

  test("the picker makes a branch on the spot, and picks it", async () => {
    const shell = await renderFrame();
    await onNeon(shell);
    await clickAction("☁ Pick a Neon branch");
    await screen.findByTestId("neon-target-staging");
    fireEvent.change(screen.getByTestId("neon-create-name"), { target: { value: "demo" } });
    fireEvent.change(screen.getByLabelText("New branch lifetime"), { target: { value: "24" } });
    fireEvent.click(screen.getByTestId("neon-create-submit"));
    const made = (await screen.findByTestId("neon-target-demo")) as HTMLInputElement;
    await waitFor(() => expect(made.checked).toBe(true));
    const branch = (await shell.neon.branches(shell.workspaceId)).branches.find(
      (b) => b.name === "demo",
    );
    expect(branch?.owned).toBe(true);
    fireEvent.click(screen.getByTestId("neon-target-save"));
    await waitFor(() => expect(headerDetail("Neon")).toBe("demo"));

    // A name the project has is refused, and says so.
    await clickAction("Change branch ▾");
    await screen.findByTestId("neon-target-demo");
    fireEvent.change(screen.getByTestId("neon-create-name"), { target: { value: "demo" } });
    fireEvent.click(screen.getByTestId("neon-create-submit"));
    await waitFor(() =>
      expect(screen.getByTestId("neon-create-branch").textContent).toContain("already has"),
    );
  });

  test("the header shows where it runs on Neon, and Start asks only without a target", async () => {
    const started: unknown[] = [];
    const shell = await renderFrame({ onStartOn: (_env, target) => started.push(target) });
    await onNeon(shell);
    // No target yet: the header and the tree say so, before any Start.
    await waitFor(() => expect(headerDetail("Neon")).toBe("no branch yet"));
    expect(action("☁ Pick a Neon branch")).toBeDefined();

    await clickAction("Start");
    const dialog = await screen.findByTestId("neon-target");
    expect(dialog.textContent).toContain("headwaters and unity-catalog as functions");
    fireEvent.click(await screen.findByTestId("neon-target-staging"));
    expect(screen.getByTestId("neon-target-save").textContent).toBe("Save and start");
    fireEvent.click(screen.getByTestId("neon-target-save"));
    await waitFor(() => expect(envState()).toBe("running"));
    expect(started).toEqual([{ project: null, branch: { kind: "existing", branch: "staging" } }]);
    expect(headerDetail("Neon")).toBe("staging");
    expect(header()?.textContent).toContain("local slot 0");
    // Unity Catalog runs as a function: no container of it, but its apps are there.
    await clickAction("Apps");
    expect(appButton("Catalog")).toBeDefined();
    expect(appButton("Lineage")).toBeDefined();
    fireEvent.click(appsActivity());
    openRunPanel();
    expect(screen.getByTestId("run-services").textContent).not.toContain("unitycatalog");
    // While it runs, the target can't change.
    const picker = action("Change branch ▾") as HTMLButtonElement;
    expect(picker.disabled).toBe(true);
    expect(picker.parentElement?.getAttribute("title")).toBe(
      "Stop dev to move it to another branch",
    );

    // Stopped: Start runs on the target without asking.
    fireEvent.click(action("Stop"));
    await waitFor(() => expect(envState()).toBe("stopped"));
    expect(headerDetail("Neon")).toBe("staging");
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
    await waitFor(() => expect(headerDetail("Neon")).toBe("luminescent-us / main"));
    expect(envState()).toBe("stopped");
    expect((await shell.run.settings()).neonTargets[shell.dev]).toEqual({
      project: "quiet-lake-11",
      branch: { kind: "existing", branch: "main" },
    });

    // Back to the workspace's project, on a new branch for 12 hours.
    fireEvent.click(action("Change branch ▾"));
    fireEvent.change(await screen.findByTestId("neon-target-project"), {
      target: { value: "\0workspace" },
    });
    fireEvent.click(await screen.findByTestId("neon-target-new"));
    fireEvent.change(screen.getByLabelText("Lifetime"), { target: { value: "12" } });
    fireEvent.click(screen.getByTestId("neon-target-save"));
    await waitFor(() => expect(headerDetail("Neon")).toBe("new branch"));
    await clickAction("Start");
    await waitFor(() => expect(envState()).toBe("running"));
    expect(started).toEqual([{ project: null, branch: { kind: "new", ttlHours: 12 } }]);
    // The branch made for it is its target from now on.
    expect(headerDetail("Neon")).toBe("local-dev-r1");
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
