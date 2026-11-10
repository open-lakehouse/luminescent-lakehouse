// The desktop's settings, behind the gear in the editor's activity bar: general,
// workspaces, Neon, doctor, in the real frame on fakes.

import { afterEach, beforeEach, expect, test } from "bun:test";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost } from "../host/fake-host";
import { ShellLayout } from "./ShellLayout";

beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
});
afterEach(cleanup);

function renderFrame(options: Parameters<typeof createFakeShellHost>[0] = {}) {
  const host = createFakeHost({ kind: "desktop", environment: null });
  const shell = createFakeShellHost(options);
  render(
    <Providers host={host} shell={shell}>
      <ShellLayout />
    </Providers>,
  );
  return { shell, host };
}

const gear = () => screen.getByRole("button", { name: "Settings" });

/** Open the settings (the gear) on `section`. */
async function openSettings(section: "general" | "workspaces" | "neon" | "doctor") {
  await waitFor(() => expect(gear()).toBeDefined());
  fireEvent.click(gear());
  fireEvent.click(await screen.findByTestId(`settings-${section}`));
}

test("a machine with no workspace gets local/dev, as the editor's starter", async () => {
  const { shell } = renderFrame();
  await waitFor(async () => {
    const list = await shell.state.stateWorkspaces();
    expect(list.status === "ok" && list.data.map((w) => w.name)).toEqual(["local"]);
  });
  await waitFor(() =>
    expect(
      screen.getByTestId("workspace-view").querySelector('[aria-label="Views"]'),
    ).not.toBeNull(),
  );
  await openSettings("workspaces");
  await waitFor(() =>
    expect(screen.getByTestId("workspace-picker").textContent).toContain("local"),
  );
  expect(screen.getByTestId("state-location").textContent).toBe("/fake/trestle/state.db");
});

test("existing workspaces are opened as they are, with no starter added", async () => {
  const shell = createFakeShellHost();
  await shell.state.stateCreateWorkspace("analytics", false, "dev");
  render(
    <Providers host={createFakeHost({ kind: "desktop" })} shell={shell}>
      <ShellLayout />
    </Providers>,
  );
  await openSettings("workspaces");
  await waitFor(() =>
    expect(screen.getByTestId("workspace-picker").textContent).toContain("analytics"),
  );
  expect((await shell.workspaces.list()).map((w) => w.name)).toEqual(["analytics"]);
});

test("the activity bar has the editor's views, Apps and one gear; settings open over the workspace", async () => {
  renderFrame();
  await waitFor(() => expect(gear()).toBeDefined());
  const bar = document.querySelector('nav[aria-label="Views"]') as HTMLElement;
  expect(document.querySelectorAll('nav[aria-label="Views"]')).toHaveLength(1);
  const labels = [...bar.querySelectorAll("button")].map((b) => b.getAttribute("aria-label"));
  expect(labels).toEqual(["Environments", "History", "Workspace settings", "Apps", "Settings"]);
  // No sidebar views of the desktop's, and no Modules.
  expect(screen.queryByRole("button", { name: "Doctor" }) === null).toBe(true);
  expect(screen.queryByRole("button", { name: "Modules" }) === null).toBe(true);
  // The gear opens a dialog; the environments stay in the sidebar under it.
  fireEvent.click(gear());
  expect(await screen.findByTestId("settings")).toBeDefined();
  // (The modal hides it from assistive tech, but it stays mounted.)
  expect(document.querySelector('[role="tree"][aria-label="Environments"]') !== null).toBe(true);
  fireEvent.click(screen.getByTestId("settings-doctor"));
  expect(await screen.findByText("Local environments can run")).toBeDefined();
});

test("the status bar's workspace opens the workspaces' settings", async () => {
  renderFrame();
  fireEvent.click(await screen.findByTestId("status-workspace"));
  expect(await screen.findByTestId("workspace-picker")).toBeDefined();
});

test("the Neon settings list the branches, and deletes only those trestle made", async () => {
  renderFrame();
  await openSettings("neon");
  const card = await screen.findByTestId("neon-card");
  await waitFor(() => expect(card.textContent).toContain("cool-poetry-28343550"));
  expect(screen.getByTestId("neon-production").textContent).toContain("default");
  expect(screen.queryByTestId("neon-production-delete") === null).toBe(true);
  expect(screen.queryByTestId("neon-staging-delete") === null).toBe(true);
  // No branch is "used" here any more: environments run on one.
  expect(screen.queryByTestId("neon-staging-use") === null).toBe(true);

  fireEvent.click(screen.getByTestId("neon-scratch-delete"));
  fireEvent.click(await screen.findByTestId("neon-scratch-delete-confirm"));
  await waitFor(() => expect(screen.queryByTestId("neon-scratch") === null).toBe(true));
  expect(screen.getByTestId("neon-staging")).toBeDefined();
});

test("the Neon settings make a branch on the spot, owned by trestle", async () => {
  renderFrame();
  await openSettings("neon");
  const create = await screen.findByTestId("neon-create-branch");
  expect(create.textContent).toContain("from");
  fireEvent.change(screen.getByTestId("neon-create-name"), { target: { value: "demo" } });
  fireEvent.click(screen.getByTestId("neon-create-submit"));
  const row = await screen.findByTestId("neon-demo");
  expect(row.textContent).toContain("trestle");
  expect(row.textContent).toContain("expires");
  expect(screen.getByTestId("neon-demo-delete")).toBeDefined();
});

test("a Neon CLI failure shows in the Neon settings", async () => {
  renderFrame({ neon: new Error("the Neon CLI is not signed in") });
  await openSettings("neon");
  const card = await screen.findByTestId("neon-card");
  await waitFor(() => expect(card.textContent).toContain("not signed in"));
});

test("stop on quit is a setting, keep running by default", async () => {
  const { shell } = renderFrame();
  await openSettings("general");
  const toggle = await screen.findByTestId("stop-on-quit");
  expect(toggle.getAttribute("aria-checked")).toBe("false");
  fireEvent.click(toggle);
  await waitFor(() => expect(toggle.getAttribute("aria-checked")).toBe("true"));
  expect((await shell.run.settings()).quit).toBe("stop");
});

test("what the launch's reconcile found is shown until dismissed", async () => {
  renderFrame({
    reconciled: {
      adopted: [{ id: "a", label: "local/dev #1" }],
      stale: [{ id: "b", label: "local/prod #2" }],
      orphans: ["old_dev_r3"],
    },
  });
  await openSettings("general");
  const notice = await screen.findByTestId("reconciled");
  expect(notice.textContent).toContain("local/prod #2");
  expect(notice.textContent).toContain("old_dev_r3");
  expect(notice.textContent).toContain("Still running: local/dev #1");
  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  await waitFor(() => expect(screen.queryByTestId("reconciled") === null).toBe(true));
});
