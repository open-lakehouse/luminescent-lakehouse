import { afterEach, beforeEach, expect, test } from "bun:test";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost } from "../host/fake-host";
import { WorkspacesPanel } from "./side-views";
import { CurrentWorkspaceProvider } from "./WorkspaceBar";

beforeEach(() => localStorage.clear());
afterEach(cleanup);

/** A shell with `local/dev` at revision 1, whose picker answers `pick()`. */
async function setup(pick: () => string | null) {
  const shell = createFakeShellHost({ pickDirectory: () => pick() });
  const ws = await shell.state.stateCreateWorkspace("local", false, "dev");
  if (ws.status !== "ok") throw new Error("no workspace");
  const snapshot = await shell.state.stateWorkspace(ws.data.id);
  if (snapshot.status !== "ok") throw new Error("no snapshot");
  const dev = snapshot.data.environments[0];
  await shell.state.stateCommit(dev.id);
  render(
    <Providers host={createFakeHost({ kind: "desktop" })} shell={shell}>
      <CurrentWorkspaceProvider>
        <WorkspacesPanel />
      </CurrentWorkspaceProvider>
    </Providers>,
  );
  await waitFor(() => expect(screen.getByTestId("export").hasAttribute("disabled")).toBe(false));
  return { shell, workspaceId: ws.data.id };
}

const status = () => screen.getByTestId("transfer-status").textContent;

test("a revision exports to a new directory and imports as an environment", async () => {
  let picked: string | null = "/exports";
  const { shell, workspaceId } = await setup(() => picked);

  fireEvent.click(screen.getByTestId("export"));
  fireEvent.click(await screen.findByTestId("export-revision-dev"));
  await waitFor(() => expect(status()).toBe("Exported to /exports/dev-r1"));
  expect(shell.state.exported.get("/exports/dev-r1")?.kind).toBe("environment");

  picked = "/exports/dev-r1";
  fireEvent.click(screen.getByTestId("import"));
  await waitFor(() => expect(status()).toBe("Imported dev-r1 into local as revision 1"));
  const envs = await shell.workspaces.environments(workspaceId);
  expect(envs.map((e) => [e.name, e.head])).toEqual([
    ["dev", 1],
    ["dev-r1", 1],
  ]);
});

test("a workspace export imported again is refused as a duplicate", async () => {
  let picked: string | null = "/exports";
  await setup(() => picked);

  fireEvent.click(screen.getByTestId("export"));
  fireEvent.click(await screen.findByTestId("export-workspace"));
  await waitFor(() => expect(status()).toBe("Exported to /exports/local"));

  picked = "/exports/local";
  fireEvent.click(screen.getByTestId("import"));
  await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("already exists"));
});

test("a cancelled picker changes nothing", async () => {
  const { shell } = await setup(() => null);
  fireEvent.click(screen.getByTestId("import"));
  fireEvent.click(screen.getByTestId("export"));
  fireEvent.click(await screen.findByTestId("export-workspace"));
  await new Promise((r) => setTimeout(r, 20));
  expect(screen.queryByTestId("transfer-status")).toBeNull();
  expect(shell.state.exported.size).toBe(0);
});

test("an environment without a revision can't be exported", async () => {
  const shell = createFakeShellHost({ pickDirectory: () => "/exports" });
  await shell.state.stateCreateWorkspace("local", false, "dev");
  render(
    <Providers host={createFakeHost({ kind: "desktop" })} shell={shell}>
      <CurrentWorkspaceProvider>
        <WorkspacesPanel />
      </CurrentWorkspaceProvider>
    </Providers>,
  );
  await waitFor(() => expect(screen.getByTestId("export").hasAttribute("disabled")).toBe(false));
  fireEvent.click(screen.getByTestId("export"));
  expect(await screen.findByText("no revision yet")).toBeTruthy();
  fireEvent.click(screen.getByTestId("export-workspace"));
  await waitFor(() =>
    expect(screen.getByRole("alert").textContent).toContain("has no revision; commit it first"),
  );
});
