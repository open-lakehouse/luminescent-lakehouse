// The shell in WebKit, on the mocked IPC (src/host/ipc-mock.ts): navigation,
// the doctor, and the embedded workspace editor.
import { expect, test } from "@playwright/test";

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("shell")).toBeVisible();
});

test("opens on the workspace editor with the starter environment", async ({ page }) => {
  const workspace = page.getByTestId("workspace-view");
  await expect(workspace.getByRole("navigation", { name: "Views" })).toBeVisible();
  await expect(workspace.getByText("dev", { exact: true }).first()).toBeVisible();
});

test("the doctor reports the mocked toolchain", async ({ page }) => {
  await page.getByTestId("nav-doctor").click();
  await expect(page.getByTestId("doctor-summary")).toContainText("Ready");
  await expect(page.getByTestId("doctor-docker")).toContainText("29.5.2");
  await expect(page.getByTestId("doctor-daemon")).toContainText("OrbStack 28.5.2");
  await expect(page.getByTestId("doctor-neon")).toContainText("not found");
  await expect(page.getByTestId("doctor-path")).toContainText("/opt/homebrew/bin");

  await page.getByTestId("nav-workspace").click();
  await expect(page.getByTestId("workspace-view")).toBeVisible();
});

test("the theme toggle switches the document theme", async ({ page }) => {
  const html = page.locator("html");
  const wasDark = await html.evaluate((el) => el.classList.contains("dark"));
  await page.getByRole("button", { name: wasDark ? "Light theme" : "Dark theme" }).click();
  await expect(html).toHaveClass(wasDark ? /^(?!.*\bdark\b)/ : /\bdark\b/);
});

test("commits a revision of the draft", async ({ page }) => {
  await page.getByRole("button", { name: "History" }).click();
  await expect(page.getByText("No revisions yet.")).toBeVisible();
  await page.getByLabel("Revision message").fill("first cut");
  await page.getByRole("button", { name: "Commit" }).click();
  // trestle's shared state keeps no commit message yet; the revision is numbered.
  const revisions = page.getByRole("list", { name: "Revisions of dev" });
  await expect(revisions).toContainText("#1");
  await expect(page.getByText("same as #1")).toBeVisible();
});

test("the workspace comes from the shared state, with a picker and its location", async ({
  page,
}) => {
  await expect(page.getByTestId("workspace-picker")).toContainText("local");
  await expect(page.getByTestId("state-location")).toHaveText("/fake/trestle/state.db");

  await page.getByTestId("new-workspace").click();
  const dialog = page.getByRole("dialog", { name: "New workspace" });
  await dialog.getByLabel("Name").fill("demo");
  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(dialog).toBeHidden();
  await expect(page.getByTestId("workspace-picker")).toContainText("demo");

  await page.getByTestId("workspace-picker").click();
  await expect(page.getByRole("option", { name: "local" })).toBeVisible();
  await expect(page.getByRole("option", { name: "demo" })).toBeVisible();
});

test("Run starts the committed revision and stops it", async ({ page }) => {
  await page.getByRole("button", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit" }).click();
  await expect(page.getByRole("list", { name: "Revisions of dev" })).toContainText("#1");

  await page.getByTestId("nav-run").click();
  const state = page.getByTestId("run-dev-state");
  await expect(state).toHaveText("stopped");
  await page.getByTestId("run-dev-start").click();
  await expect(state).toHaveText("running");
  await expect(page.getByTestId("gateway-url")).toContainText("http://localhost:9080");

  await page.getByRole("button", { name: "dev #1 containers" }).click();
  await expect(page.getByTestId("run-services")).toContainText("unity-catalog");

  await page.getByTestId("run-dev-stop").click();
  await expect(state).toHaveText("stopped");
});

test("Use makes a Neon branch the current environment, and a start replaces it", async ({
  page,
}) => {
  await page.getByTestId("nav-run").click();
  await expect(page.getByTestId("neon-card")).toContainText("cool-poetry-28343550");
  await page.getByTestId("neon-staging-use").click();
  await expect(page.getByTestId("neon-staging-current")).toBeVisible();

  // Starting a local instance makes it the current environment instead.
  await page.getByTestId("nav-workspace").click();
  await page.getByRole("button", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit" }).click();
  await page.getByTestId("nav-run").click();
  await page.getByTestId("run-dev-start").click();
  await expect(page.getByTestId("run-dev-r1-current")).toBeVisible();
  await expect(page.getByTestId("neon-staging-use")).toBeVisible();
  await page.getByTestId("run-dev-stop").click();
  await expect(page.getByTestId("run-dev-state")).toHaveText("stopped");
});

test("History compares two revisions", async ({ page }) => {
  await page.getByRole("button", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit" }).click();
  const revisions = page.getByRole("list", { name: "Revisions of dev" });
  await expect(revisions).toContainText("#1");

  // Add a module to the draft, as `trestle env` would, and commit it as #2.
  await page.evaluate(async () => {
    const state = (
      window as unknown as { __LL_FAKE_STATE__: import("../src/host/fake-state").FakeState }
    ).__LL_FAKE_STATE__;
    const list = await state.stateWorkspaces();
    if (list.status !== "ok") throw new Error("no workspaces");
    const ws = await state.stateWorkspace(list.data[0].id);
    if (ws.status !== "ok") throw new Error("no workspace");
    const dev = ws.data.environments[0];
    const selection = dev.selection as { modules: string[] };
    await state.stateSetSelection(dev.id, {
      ...selection,
      modules: [...selection.modules, "jaeger"],
    });
  });
  await expect(page.getByRole("button", { name: "Diff" })).toBeEnabled();
  await page.getByRole("button", { name: "Diff" }).click();
  await expect(page.getByRole("list", { name: "Changes" })).toContainText("module jaeger");

  await page.getByRole("button", { name: "Commit" }).click();
  await expect(revisions).toContainText("#2");
  // The compare button shows on hover.
  await revisions.getByRole("listitem").filter({ hasText: "#2" }).hover();
  await page.getByRole("button", { name: "Compare #1 with #2" }).click();
  await expect(page.getByText("#1 → #2").first()).toBeVisible();
  await expect(page.getByRole("list", { name: "Changes" })).toContainText("module jaeger");
});

test("exports a revision and imports it as an environment", async ({ page }) => {
  await page.getByRole("button", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit" }).click();
  await expect(page.getByRole("list", { name: "Revisions of dev" })).toContainText("#1");

  await page.getByTestId("export").click();
  const dialog = page.getByTestId("export-dialog");
  await dialog.getByTestId("export-revision-dev").click();
  await expect(page.getByTestId("transfer-status")).toHaveText("Exported to /fake/exports/dev-r1");

  // The picker now answers the exported directory.
  await page.evaluate(() => {
    (window as { __LL_PICK__?: string }).__LL_PICK__ = "/fake/exports/dev-r1";
  });
  await page.getByTestId("import").click();
  await expect(page.getByTestId("transfer-status")).toHaveText(
    "Imported dev-r1 into local as revision 1",
  );
  await expect(
    page.getByTestId("workspace-view").getByText("dev-r1", { exact: true }).first(),
  ).toBeVisible();
});

test("removes an environment from the explorer after confirming", async ({ page }) => {
  const tree = page.getByRole("tree", { name: "Environments" });
  await page.getByRole("button", { name: "New environment" }).click();
  await page.getByLabel("Environment name").fill("prod");
  await page.getByLabel("Environment name").press("Enter");
  const prod = tree.getByRole("treeitem").filter({ hasText: "prod" });
  await expect(prod).toBeVisible();

  // Cancel keeps it.
  await prod.hover();
  await prod.getByRole("button", { name: "Remove" }).click();
  const confirm = tree.getByRole("alertdialog", { name: "Remove" });
  await expect(confirm).toContainText("Remove prod from the workspace?");
  await confirm.getByRole("button", { name: "Cancel" }).click();
  await expect(confirm).toBeHidden();
  await expect(prod).toBeVisible();

  // So does Escape.
  await prod.hover();
  await prod.getByRole("button", { name: "Remove" }).click();
  await expect(confirm).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(confirm).toBeHidden();
  await expect(prod).toBeVisible();

  // Confirming removes it, from the shared state too.
  await prod.hover();
  await prod.getByRole("button", { name: "Remove" }).click();
  await confirm.getByRole("button", { name: "Remove" }).click();
  await expect(prod).toBeHidden();
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const state = (
          window as unknown as { __LL_FAKE_STATE__: import("../src/host/fake-state").FakeState }
        ).__LL_FAKE_STATE__;
        const list = await state.stateWorkspaces();
        if (list.status !== "ok") throw new Error("no workspaces");
        const ws = await state.stateWorkspace(list.data[0].id);
        if (ws.status !== "ok") throw new Error("no workspace");
        return ws.data.environments.map((e) => e.name);
      }),
    )
    .toEqual(["dev"]);
});
