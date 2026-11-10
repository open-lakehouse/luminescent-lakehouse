// The Apps view in the frame: the running instances, each one's apps (Catalog and
// Lineage on its services, its UIs with a session of their own), and the way
// there from an environment's header, on the fake state and the real editor.

import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import { instanceApps, runningInstances, selectedInstance } from "./apps";
import {
  action,
  appButton,
  appsActivity,
  appsSidebar,
  clickAction,
  commit,
  editorHidden,
  envState,
  renderFrame,
  runFromPalette,
  stubRect,
} from "./frame.testing";
import type { EmbedBounds, EnvironmentRun, InstanceStatus, ServiceRow } from "./shell-host";

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
});
afterEach(cleanup);

const MLFLOW = {
  moduleId: "mlflow",
  name: "MLflow",
  url: "http://local-dev-r1.localhost:9080/mlflow/",
};

describe("the model", () => {
  const row = (module: string): ServiceRow =>
    ({ module, state: "running", health: "healthy", ready: true }) as ServiceRow;
  const instance = (over: Partial<InstanceStatus>): InstanceStatus =>
    ({
      id: "i1",
      revision: 1,
      desired: "running",
      slot: 0,
      branch: null,
      gatewayUrl: null,
      services: [row("unitycatalog")],
      ...over,
    }) as InstanceStatus;
  const env = (name: string, instances: InstanceStatus[]): EnvironmentRun =>
    ({ id: name, name, head: 1, instances }) as EnvironmentRun;

  test("lists the instances wanted running, by environment, newest first", () => {
    const run = {
      environments: [
        env("prod", [instance({ id: "p1" })]),
        env("dev", [
          instance({ id: "d1", revision: 1 }),
          instance({ id: "d2", revision: 2 }),
          instance({ id: "d0", revision: 3, desired: "stopped", services: [] }),
        ]),
      ],
    } as Parameters<typeof runningInstances>[0];
    expect(runningInstances(run).map((r) => r.label)).toEqual(["dev #2", "dev #1", "prod #1"]);
  });

  test("an instance's apps: the data views it serves, then its UIs", () => {
    const uc = instance({ services: [row("unity-catalog-oss")] });
    // Keyed by the API: the implementation's module counts only as itself.
    expect(instanceApps(uc, []).map((a) => a.id)).toEqual([]);
    const both = instance({
      services: [],
      branch: { name: "staging", functions: ["unity-catalog", "headwaters"] },
    } as unknown as Partial<InstanceStatus>);
    expect(instanceApps(both, [MLFLOW]).map((a) => a.label)).toEqual([
      "Catalog",
      "Lineage",
      "MLflow",
    ]);
  });

  test("picks the picked instance, else the current one, else the first one up", () => {
    const running = runningInstances({
      environments: [
        env("dev", [instance({ id: "d1", services: [{ ...row("x"), ready: false }] })]),
        env("prod", [instance({ id: "p1" })]),
      ],
    } as Parameters<typeof runningInstances>[0]);
    expect(selectedInstance(running, undefined, undefined)?.instance.id).toBe("p1");
    expect(selectedInstance(running, undefined, "d1")?.instance.id).toBe("d1");
    expect(selectedInstance(running, "p1", "d1")?.instance.id).toBe("p1");
    expect(selectedInstance(running, "gone", undefined)?.instance.id).toBe("p1");
  });
});

test("with nothing running, Apps says so and leads back to the environments", async () => {
  await renderFrame();
  fireEvent.click(appsActivity());
  await screen.findByTestId("apps-empty");
  expect(editorHidden()).toBe(true);
  expect(appsSidebar().textContent).toContain("Nothing is running.");
  fireEvent.click(screen.getByRole("button", { name: "Show the environments" }));
  await waitFor(() => expect(screen.queryByTestId("apps-empty") === null).toBe(true));
  expect(editorHidden()).toBe(false);
});

test("the header's Apps opens the instance's Catalog, which makes it current", async () => {
  const shell = await renderFrame();
  await commit(shell);
  // Started earlier (the CLI, or before a restart): running, but not current.
  const started = await shell.state.runStart(shell.dev);
  if (started.status !== "ok") throw new Error("start");
  await waitFor(() => expect(envState()).toBe("running"));
  expect(shell.host.environment.current()).toBeNull();

  await clickAction("Apps");
  await screen.findByTestId("apps-main-catalog");
  expect(editorHidden()).toBe(true);
  const selected = screen.getByTestId(`apps-instance-${started.data.id}`);
  expect(selected.getAttribute("aria-current")).toBe("true");
  expect(selected.textContent).toContain("dev #1");
  expect(selected.textContent).toContain("local slot 0");
  await waitFor(() => expect(shell.host.environment.current()?.id).toBe(started.data.id));
  // The fake binds nothing: the view says what doesn't serve it, by name.
  await waitFor(() =>
    expect(screen.getByTestId("unbound-unity-catalog").textContent).toContain("dev #1"),
  );
  // A revision without headwaters has no Lineage.
  expect(within(appsSidebar()).queryByRole("button", { name: "Lineage" }) === null).toBe(true);

  // The activity bar's item goes back to the editor, which kept its tab.
  fireEvent.click(appsActivity());
  await waitFor(() => expect(editorHidden()).toBe(false));
  expect(envState()).toBe("running");
});

test("a starting instance is listed, and its apps wait until it is up", async () => {
  const shell = await renderFrame();
  await commit(shell);
  const started = await shell.state.runStart(shell.dev);
  if (started.status !== "ok") throw new Error("start");
  await waitFor(() => expect(envState()).toBe("running"));
  const all = (over: Partial<ServiceRow>) =>
    started.data.services.map((s) => ({ ...s, ...over }) as ServiceRow);
  shell.state.setServices(started.data.id, all({ health: "starting", ready: false }));
  await waitFor(() => expect(envState()).toBe("starting"));
  expect((action("Apps") as HTMLButtonElement).disabled).toBe(true);

  fireEvent.click(appsActivity());
  await waitFor(() => expect(screen.getByText("dev #1 is starting…")).toBeDefined());
  expect(within(appsSidebar()).queryByRole("button", { name: "Catalog" }) === null).toBe(true);

  shell.state.setServices(started.data.id, started.data.services);
  await screen.findByTestId("apps-main-catalog");
});

test("two environments running: the selector moves the apps to the one picked", async () => {
  const shell = await renderFrame();
  await commit(shell);
  const prod = await shell.state.stateCreateEnvironment(shell.workspaceId, "prod", null);
  if (prod.status !== "ok") throw new Error("prod");
  await commit(shell, prod.data.id);
  const dev = await shell.state.runStart(shell.dev);
  const prodRun = await shell.state.runStart(prod.data.id);
  if (dev.status !== "ok" || prodRun.status !== "ok") throw new Error("start");
  await waitFor(() => expect(envState()).toBe("running"));

  await clickAction("Apps");
  await waitFor(() => expect(shell.host.environment.current()?.id).toBe(dev.data.id));
  fireEvent.click(screen.getByTestId(`apps-instance-${prodRun.data.id}`));
  await waitFor(() => expect(shell.host.environment.current()?.id).toBe(prodRun.data.id));
  expect(screen.getByTestId(`apps-instance-${prodRun.data.id}`).getAttribute("aria-current")).toBe(
    "true",
  );
  await screen.findByTestId("apps-main-catalog");

  // ⌘K reaches an app of either one.
  await runFromPalette("Apps: Catalog on dev #1");
  await waitFor(() => expect(shell.host.environment.current()?.id).toBe(dev.data.id));
});

test("MLflow shows in place, hides under an overlay, pops out and comes back", async () => {
  const opened: [string, string][] = [];
  const embeds: [string, string, EmbedBounds | null][] = [];
  const shell = await renderFrame({
    serviceUis: () => [MLFLOW],
    onOpenWindow: (instance, module) => opened.push([instance, module]),
    onEmbed: (instance, module, bounds) => embeds.push([instance, module, bounds]),
  });
  await commit(shell);
  const started = await shell.state.runStart(shell.dev);
  if (started.status !== "ok") throw new Error("start");
  const id = started.data.id;
  await waitFor(() => expect(envState()).toBe("running"));
  // Not a header action, nor a tab of the environment.
  expect(screen.queryByRole("tab", { name: "MLflow" }) === null).toBe(true);

  const area = { left: 240, top: 120, width: 800, height: 500 };
  const restore = stubRect('[data-testid="service-embed"]', area);
  try {
    await clickAction("Apps");
    fireEvent.click(await waitFor(() => appButton("MLflow")));
    const shown = { x: 240, y: 120, width: 800, height: 500 };
    await waitFor(() => expect(embeds.at(-1)).toEqual([id, "mlflow", shown]));

    // Something pops over it: the native view would paint over it, so it hides.
    const menu = document.createElement("div");
    menu.setAttribute("role", "menu");
    document.body.append(menu);
    const restoreMenu = stubRect('[role="menu"]', { left: 300, top: 200, width: 120, height: 80 });
    await waitFor(() => expect(embeds.at(-1)).toEqual([id, "mlflow", null]));
    menu.remove();
    restoreMenu();
    await waitFor(() => expect(embeds.at(-1)).toEqual([id, "mlflow", shown]));

    // Pop out: the window replaces it.
    fireEvent.click(screen.getByRole("button", { name: "Pop out" }));
    await waitFor(() => expect(opened).toEqual([[id, "mlflow"]]));
    expect(embeds.at(-1)).toEqual([id, "mlflow", null]);
    expect(screen.getByText("MLflow is open in its own window.")).toBeDefined();

    fireEvent.click(screen.getByRole("button", { name: "Show here" }));
    await waitFor(() => expect(embeds.at(-1)).toEqual([id, "mlflow", shown]));

    // Back to the editor hides it.
    fireEvent.click(appsActivity());
    await waitFor(() => expect(embeds.at(-1)).toEqual([id, "mlflow", null]));
  } finally {
    restore();
  }
});

test("where the host can't embed (Linux), MLflow opens in a window", async () => {
  const opened: [string, string][] = [];
  const embeds: unknown[] = [];
  const shell = await renderFrame({
    embeds: false,
    serviceUis: () => [MLFLOW],
    onOpenWindow: (instance, module) => opened.push([instance, module]),
    onEmbed: (...call) => embeds.push(call),
  });
  await commit(shell);
  const started = await shell.state.runStart(shell.dev);
  if (started.status !== "ok") throw new Error("start");
  await waitFor(() => expect(envState()).toBe("running"));
  await clickAction("Apps");
  fireEvent.click(await waitFor(() => appButton("MLflow")));
  await waitFor(() =>
    expect(screen.getByText("MLflow opens in its own window on this platform.")).toBeDefined(),
  );
  fireEvent.click(screen.getByRole("button", { name: "Open window" }));
  await waitFor(() => expect(opened).toEqual([[started.data.id, "mlflow"]]));
  // Still the same: nothing to bring back, and nothing ever embedded.
  expect(screen.queryByRole("button", { name: "Show here" }) === null).toBe(true);
  expect(embeds).toEqual([]);
});
