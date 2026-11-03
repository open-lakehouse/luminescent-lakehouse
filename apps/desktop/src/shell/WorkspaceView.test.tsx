import { afterEach, beforeEach, expect, test } from "bun:test";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost } from "../host/fake-host";
import { CurrentWorkspaceProvider } from "./WorkspaceBar";
import { WorkspaceView } from "./WorkspaceView";

beforeEach(() => localStorage.clear());
afterEach(cleanup);

function renderView(shell = createFakeShellHost()) {
  render(
    <Providers host={createFakeHost({ kind: "desktop" })} shell={shell}>
      <CurrentWorkspaceProvider>
        <WorkspaceView />
      </CurrentWorkspaceProvider>
    </Providers>,
  );
  return shell;
}

test("a machine with no workspace gets local/dev, as the editor's starter", async () => {
  const shell = renderView();
  await waitFor(async () => {
    const list = await shell.state.stateWorkspaces();
    expect(list.status === "ok" && list.data.map((w) => w.name)).toEqual(["local"]);
  });
  await waitFor(() =>
    expect(screen.getByTestId("workspace-picker").textContent).toContain("local"),
  );
  expect(screen.getByTestId("state-location").textContent).toBe("/fake/trestle/state.db");
  await waitFor(() =>
    expect(
      screen.getByTestId("workspace-view").querySelector('[aria-label="Views"]'),
    ).not.toBeNull(),
  );
});

test("existing workspaces are opened as they are, with no starter added", async () => {
  const shell = createFakeShellHost();
  await shell.state.stateCreateWorkspace("analytics", false, "dev");
  renderView(shell);
  await waitFor(() =>
    expect(screen.getByTestId("workspace-picker").textContent).toContain("analytics"),
  );
  expect((await shell.workspaces.list()).map((w) => w.name)).toEqual(["analytics"]);
});
