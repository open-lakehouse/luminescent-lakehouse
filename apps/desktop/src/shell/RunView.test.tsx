import { afterEach, beforeEach, describe, expect, test } from "bun:test";
import { createFakeHost } from "@open-lakehouse/host/testing";
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

async function renderRun(): Promise<FakeShellHost> {
  const shell = createFakeShellHost();
  const ws = await shell.workspaces.create("local", "dev");
  render(
    <Providers host={createFakeHost({ kind: "desktop" })} shell={shell}>
      <CurrentWorkspaceProvider>
        <RunView />
      </CurrentWorkspaceProvider>
    </Providers>,
  );
  await screen.findByTestId("run-dev");
  expect(ws.name).toBe("local");
  return shell;
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

  fireEvent.click(screen.getByRole("button", { name: "dev containers" }));
  expect(screen.getByTestId("run-services").textContent).toContain("unity-catalog");

  fireEvent.click(screen.getByTestId("run-dev-stop"));
  await waitFor(() => expect(screen.getByTestId("run-dev-state").textContent).toBe("stopped"));
  expect(screen.queryByTestId("gateway-url")).toBeNull();
});
