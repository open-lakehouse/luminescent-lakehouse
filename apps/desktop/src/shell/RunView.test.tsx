import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { createFakeHost, type FakeHost } from "@open-lakehouse/host/testing";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost, type FakeShellHost } from "../host/fake-host";
import { RunView, runState } from "./RunView";
import type { InstanceStatus, ServiceRow } from "./shell-host";
import { CurrentWorkspaceProvider } from "./WorkspaceBar";

beforeEach(() => localStorage.clear());
afterEach(cleanup);

const row = (over: Partial<ServiceRow> = {}): ServiceRow => ({
  service: "postgres",
  state: "running",
  health: "healthy",
  status: "Up",
  exitCode: 0,
  ports: [],
  ready: true,
  ...over,
});

const instance = (over: Partial<InstanceStatus> = {}): InstanceStatus => ({
  id: "i",
  revision: 1,
  desired: "running",
  slot: 0,
  project: "local_dev_r1",
  gatewayUrl: "http://localhost:9080",
  services: [row()],
  ...over,
});

describe("runState", () => {
  test("from what is wanted and what Docker reports", () => {
    expect(runState(undefined)).toBe("stopped");
    expect(runState(instance({ desired: "stopped", services: [] }))).toBe("stopped");
    expect(runState(instance({ desired: "stopped" }))).toBe("stopping");
    expect(runState(instance())).toBe("running");
    expect(runState(instance({ services: [] }))).toBe("starting");
    expect(runState(instance({ services: [row({ health: "starting", ready: false })] }))).toBe(
      "starting",
    );
    expect(runState(instance({ services: [row({ health: "unhealthy", ready: false })] }))).toBe(
      "unhealthy",
    );
    expect(
      runState(instance({ services: [row({ state: "exited", exitCode: 1, ready: false })] })),
    ).toBe("unhealthy");
    // A one-shot job that finished doesn't keep the instance from running.
    expect(
      runState(
        instance({
          services: [row(), row({ service: "gateway-certs", state: "exited", ready: false })],
        }),
      ),
    ).toBe("running");
  });
});

async function renderRun(
  options: Parameters<typeof createFakeShellHost>[0] = {},
): Promise<FakeShellHost & { host: FakeHost }> {
  const host = createFakeHost({ kind: "desktop", environment: null });
  const shell = createFakeShellHost({
    onUse: (instance) => host.setEnvironment({ id: instance, name: "dev" }),
    onUseNeon: (branch) => host.setEnvironment({ id: branch.id, name: branch.name }),
    ...options,
  });
  const ws = await shell.workspaces.create("local", "dev");
  render(
    <Providers host={host} shell={shell}>
      <CurrentWorkspaceProvider>
        <RunView />
      </CurrentWorkspaceProvider>
    </Providers>,
  );
  await screen.findByTestId("run-dev");
  expect(ws.name).toBe("local");
  return Object.assign(shell, { host });
}

test("an environment without a revision can't start", async () => {
  await renderRun();
  expect(screen.getByTestId("run-dev-state").textContent).toBe("stopped");
  expect((screen.getByTestId("run-dev-start") as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText("no revision yet")).toBeDefined();
});

test("start runs the head revision, shows its gateway, stop takes it down", async () => {
  const shell = await renderRun();
  const [ws] = await shell.workspaces.list();
  const snapshot = await shell.state.stateWorkspace(ws?.id ?? "");
  if (snapshot.status !== "ok") throw new Error("snapshot");
  await shell.state.stateCommit(snapshot.data.environments[0]?.id ?? "");

  const start = await waitFor(() => {
    const button = screen.getByTestId("run-dev-start") as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    return button;
  });
  fireEvent.click(start);
  await waitFor(() => expect(screen.getByTestId("run-dev-state").textContent).toBe("running"));
  expect(screen.getByTestId("gateway-url").textContent).toContain("http://localhost:9080");
  // What starts becomes the current environment: the views use its services.
  await waitFor(() => expect(screen.getByTestId("run-dev-current")).toBeDefined());
  expect(screen.queryByTestId("run-dev-use")).toBeNull();

  fireEvent.click(screen.getByRole("button", { name: "dev containers" }));
  expect(screen.getByTestId("run-services").textContent).toContain("unity-catalog");

  fireEvent.click(screen.getByTestId("run-dev-stop"));
  await waitFor(() => expect(screen.getByTestId("run-dev-state").textContent).toBe("stopped"));
  expect(screen.queryByTestId("gateway-url")).toBeNull();
});

test("a running instance that isn't current offers Use", async () => {
  const shell = await renderRun();
  const [ws] = await shell.workspaces.list();
  const snapshot = await shell.state.stateWorkspace(ws?.id ?? "");
  if (snapshot.status !== "ok") throw new Error("snapshot");
  const dev = snapshot.data.environments[0]?.id ?? "";
  await shell.state.stateCommit(dev);
  // Started earlier (the CLI, or before a restart): running, but not current.
  await shell.state.runStart(dev);
  await waitFor(() => expect(screen.getByTestId("run-dev-state").textContent).toBe("running"));
  const use = await screen.findByTestId("run-dev-use");
  fireEvent.click(use);
  await waitFor(() => expect(screen.getByTestId("run-dev-current")).toBeDefined());
  expect(shell.host.environment.current()?.name).toBe("dev");
});

test("a Neon branch becomes the current environment", async () => {
  await renderRun();
  const card = await screen.findByTestId("neon-card");
  await waitFor(() => expect(card.textContent).toContain("cool-poetry-28343550"));
  expect(screen.getByTestId("neon-production").textContent).toContain("default");
  fireEvent.click(screen.getByTestId("neon-staging-use"));
  await waitFor(() => expect(screen.getByTestId("neon-staging-current")).toBeDefined());
  expect(screen.queryByTestId("neon-staging-use")).toBeNull();
  // The other branch still offers Use.
  expect(screen.getByTestId("neon-production-use")).toBeDefined();
});

test("a Neon CLI failure shows on the card", async () => {
  await renderRun({ neon: new Error("the Neon CLI is not signed in") });
  const card = await screen.findByTestId("neon-card");
  await waitFor(() => expect(card.textContent).toContain("not signed in"));
  // The environments still render.
  expect(screen.getByTestId("run-dev")).toBeDefined();
});

/** `local/dev` committed; returns its environment id. */
async function committedDev(shell: FakeShellHost): Promise<string> {
  const [ws] = await shell.workspaces.list();
  const snapshot = await shell.state.stateWorkspace(ws?.id ?? "");
  if (snapshot.status !== "ok") throw new Error("snapshot");
  const dev = snapshot.data.environments[0]?.id ?? "";
  await shell.state.stateCommit(dev);
  return dev;
}

const state = () => screen.getByTestId("run-dev-state").textContent;

/** Click Start once the environment's revision is there. */
async function clickStart(env: string) {
  const start = await waitFor(() => {
    const button = screen.getByTestId(`run-${env}-start`) as HTMLButtonElement;
    expect(button.disabled).toBe(false);
    return button;
  });
  fireEvent.click(start);
}

describe("lifecycle states", () => {
  test("no Docker: the containers are unknown, and Start says why", async () => {
    const shell = await renderRun();
    await committedDev(shell);
    shell.state.setEngineProblem("Cannot connect to the Docker daemon");
    await clickStart("dev");
    await waitFor(() =>
      expect(screen.getAllByRole("alert").map((a) => a.textContent)).toContainEqual(
        expect.stringContaining("Cannot connect to the Docker daemon"),
      ),
    );
    expect(state()).toBe("stopped");
  });

  test("starting, healthy, then degraded, each from Docker's events", async () => {
    const shell = await renderRun();
    const dev = await committedDev(shell);
    const started = await shell.state.runStart(dev);
    if (started.status !== "ok") throw new Error("start");
    const id = started.data.id;
    await waitFor(() => expect(state()).toBe("running"));

    shell.state.setServices(id, [
      { ...started.data.services[0], health: "starting", ready: false } as ServiceRow,
    ]);
    await waitFor(() => expect(state()).toBe("starting"));

    shell.state.setServices(id, [
      { ...started.data.services[0], health: "unhealthy", ready: false } as ServiceRow,
    ]);
    await waitFor(() => expect(state()).toBe("unhealthy"));

    shell.state.setServices(id, started.data.services);
    await waitFor(() => expect(state()).toBe("running"));
  });

  test("a port conflict names the port and its owner", async () => {
    const shell = await renderRun();
    await committedDev(shell);
    shell.state.failNextStart({ kind: "portsBusy", ports: ["5432 (ssh)"] });
    await clickStart("dev");
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("5432 (ssh)"));
    expect(state()).toBe("stopped");
  });
});

test("one at a time stops the workspace's other environment on start", async () => {
  const shell = await renderRun();
  const [ws] = await shell.workspaces.list();
  const dev = await committedDev(shell);
  const prod = await shell.state.stateCreateEnvironment(ws?.id ?? "", "prod", null);
  if (prod.status !== "ok") throw new Error("prod");
  await shell.state.stateCommit(prod.data.id);
  await shell.state.runStart(dev);
  await waitFor(() => expect(state()).toBe("running"));

  fireEvent.click(await screen.findByTestId("one-at-a-time"));
  await waitFor(() =>
    expect(screen.getByTestId("one-at-a-time").getAttribute("aria-checked")).toBe("true"),
  );
  await clickStart("prod");
  await waitFor(() => expect(screen.getByTestId("run-prod-state").textContent).toBe("running"));
  await waitFor(() => expect(state()).toBe("stopped"));
  expect((await shell.run.settings()).oneAtATime).toEqual([ws?.id ?? ""]);
});

test("stop on quit is a setting, keep running by default", async () => {
  const shell = await renderRun();
  const toggle = await screen.findByTestId("stop-on-quit");
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  fireEvent.click(toggle);
  await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("true"));
  expect((await shell.run.settings()).quit).toBe("stop");
});

test("what the launch's reconcile found is shown until dismissed", async () => {
  sessionStorage.clear();
  await renderRun({
    reconciled: {
      adopted: [{ id: "a", label: "local/dev #1" }],
      stale: [{ id: "b", label: "local/prod #2" }],
      orphans: ["old_dev_r3"],
    },
  });
  const notice = await screen.findByTestId("reconciled");
  expect(notice.textContent).toContain("local/prod #2");
  expect(notice.textContent).toContain("old_dev_r3");
  expect(notice.textContent).toContain("Still running: local/dev #1");
  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  await waitFor(() => expect(screen.queryByTestId("reconciled")).toBeNull());
});
