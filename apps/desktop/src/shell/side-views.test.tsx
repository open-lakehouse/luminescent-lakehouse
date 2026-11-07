// The desktop's views in the editor's activity bar: workspaces, Neon, doctor,
// app settings, in the real frame on fakes.

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

const view = (label: string) => screen.getByRole("button", { name: label });

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
  fireEvent.click(view("Workspaces"));
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
  await waitFor(() => expect(view("Workspaces")).toBeDefined());
  fireEvent.click(view("Workspaces"));
  await waitFor(() =>
    expect(screen.getByTestId("workspace-picker").textContent).toContain("analytics"),
  );
  expect((await shell.workspaces.list()).map((w) => w.name)).toEqual(["analytics"]);
});

test("the editor's activity bar carries the desktop's views, and no second rail", async () => {
  renderFrame();
  await waitFor(() => expect(view("Doctor")).toBeDefined());
  expect(document.querySelectorAll('nav[aria-label="Views"]')).toHaveLength(1);
  expect(screen.queryByRole("navigation", { name: "Shell views" }) === null).toBe(true);
  fireEvent.click(view("Doctor"));
  expect(await screen.findByText("Local environments can run")).toBeDefined();
});

test("the Neon view lists the branches, and deletes only those trestle made", async () => {
  renderFrame();
  await waitFor(() => expect(view("Neon")).toBeDefined());
  fireEvent.click(view("Neon"));
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

test("a Neon CLI failure shows in the Neon view", async () => {
  renderFrame({ neon: new Error("the Neon CLI is not signed in") });
  await waitFor(() => expect(view("Neon")).toBeDefined());
  fireEvent.click(view("Neon"));
  const card = await screen.findByTestId("neon-card");
  await waitFor(() => expect(card.textContent).toContain("not signed in"));
});

test("stop on quit is a setting, keep running by default", async () => {
  const { shell } = renderFrame();
  await waitFor(() => expect(view("App settings")).toBeDefined());
  fireEvent.click(view("App settings"));
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
  await waitFor(() => expect(view("App settings")).toBeDefined());
  fireEvent.click(view("App settings"));
  const notice = await screen.findByTestId("reconciled");
  expect(notice.textContent).toContain("local/prod #2");
  expect(notice.textContent).toContain("old_dev_r3");
  expect(notice.textContent).toContain("Still running: local/dev #1");
  fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
  await waitFor(() => expect(screen.queryByTestId("reconciled") === null).toBe(true));
});
