// The shell in WebKit, on the mocked IPC (src/host/ipc-mock.ts): the one frame
// (env-editor's workspace shell with the desktop's views), the doctor, each
// environment's run controls and sub-tabs, and the Apps view.
import { expect, type Page, test } from "@playwright/test";

/** The settings (the gear at the bottom of the activity bar), on `section`. */
async function openSettings(page: Page, section: "general" | "workspaces" | "neon" | "doctor") {
  await page
    .getByRole("navigation", { name: "Views" })
    .getByRole("button", { name: "Settings", exact: true })
    .click();
  await page.getByTestId(`settings-${section}`).click();
}

/** Close the settings, back to the workspace. */
async function closeSettings(page: Page) {
  // A dialog opened from the settings (export) closes first.
  await expect
    .poll(async () => {
      await page.keyboard.press("Escape");
      return page.getByTestId("settings").count();
    })
    .toBe(0);
}

/** The open environment's sub-tabs (Diagram, Files, Changes, Logs). */
const subTabs = (page: Page) => page.getByRole("tablist", { name: "Environment views" });

/** The open environment's header: the element just before its sub-tabs. */
const envHeader = (page: Page) => subTabs(page).locator("xpath=preceding-sibling::div[1]");

/** The open environment's state, as its header's dot says. */
const envState = (page: Page) => envHeader(page).locator("[data-run-state]").first();

async function commitDev(page: Page) {
  await page.getByRole("button", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit" }).click();
  await expect(page.getByRole("list", { name: "Revisions of dev" })).toContainText("#1");
}

/** The Apps view's item in the activity bar. */
const appsActivity = (page: Page) =>
  page.getByRole("navigation", { name: "Views" }).getByRole("button", { name: "Apps" });

/** The Apps view's sidebar: the running instances and the selected one's apps. */
const appsSidebar = (page: Page) => page.getByTestId("apps-sidebar");

/** Show the bottom panel's Run view. */
const openRunPanel = (page: Page) => page.getByRole("button", { name: "Run", exact: true }).click();

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
  await openSettings(page, "doctor");
  await expect(page.getByTestId("doctor-summary")).toContainText("Ready");
  await expect(page.getByTestId("doctor-docker")).toContainText("29.5.2");
  await expect(page.getByTestId("doctor-daemon")).toContainText("OrbStack 28.5.2");
  await expect(page.getByTestId("doctor-neon")).toContainText("not found");
  await expect(page.getByTestId("doctor-path")).toContainText("/opt/homebrew/bin");

  // The workspace stayed under the dialog.
  await closeSettings(page);
  await expect(page.getByRole("tree", { name: "Environments" })).toBeVisible();
});

test("one activity bar: the editor's views, Apps and the gear, no Modules", async ({ page }) => {
  const views = page.getByRole("navigation", { name: "Views" });
  await expect(views).toHaveCount(1);
  const labels = await views
    .getByRole("button")
    .evaluateAll((bs) => bs.map((b) => b.getAttribute("aria-label")));
  expect(labels).toEqual(["Environments", "History", "Workspace settings", "Apps", "Settings"]);
});

test("the theme toggle in App settings switches the document theme", async ({ page }) => {
  await openSettings(page, "general");
  const html = page.locator("html");
  const wasDark = await html.evaluate((el) => el.classList.contains("dark"));
  await page.getByRole("button", { name: wasDark ? "Light theme" : "Dark theme" }).click();
  await expect(html).toHaveClass(wasDark ? /^(?!.*\bdark\b)/ : /\bdark\b/);
  // One theme control: none in the status bar.
  await expect(page.getByRole("button", { name: /^(Light|Dark) theme$/ })).toHaveCount(1);
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
  await openSettings(page, "workspaces");
  await expect(page.getByTestId("workspace-picker")).toContainText("local");
  await expect(page.getByTestId("state-location")).toHaveText("/fake/trestle/state.db");

  await page.getByTestId("new-workspace").click();
  const dialog = page.getByRole("dialog", { name: "New workspace" });
  await dialog.getByLabel("Name").fill("demo");
  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(dialog).toBeHidden();
  // The frame opens the new workspace on its environments.
  await expect(page.getByRole("tree", { name: "Environments" })).toBeVisible();
  await page.getByTestId("status-workspace").click();
  await expect(page.getByTestId("workspace-picker")).toContainText("demo");

  await page.getByTestId("workspace-picker").click();
  await expect(page.getByRole("option", { name: "local" })).toBeVisible();
  await expect(page.getByRole("option", { name: "demo" })).toBeVisible();
});

test("Start runs the committed revision, Stop takes it down", async ({ page }) => {
  await commitDev(page);
  await expect(envState(page)).toHaveAttribute("data-run-state", "stopped");
  await envHeader(page).getByRole("button", { name: "Start" }).click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await expect(envHeader(page)).toContainText("rev 1");
  await expect(envHeader(page)).toContainText("local slot 0");

  await openRunPanel(page);
  await expect(page.getByTestId("gateway-url")).toContainText("http://localhost:9080");
  await expect(page.getByTestId("run-services")).toContainText("unitycatalog");

  await envHeader(page).getByRole("button", { name: "Stop" }).click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "stopped");
});

test("⌘K runs Start, and copies the CLI equivalent", async ({ page }) => {
  await commitDev(page);
  // What Copy CLI writes, without asking WebKit for clipboard permissions.
  await page.evaluate(() => {
    const w = window as unknown as { __copied?: string };
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: {
        writeText: async (text: string) => {
          w.__copied = text;
        },
      },
    });
  });
  const palette = page.getByRole("dialog");
  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.type("env: start dev");
  await expect(palette).toContainText("trestle up local/dev@1");
  await page.keyboard.press("Enter");
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");

  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.type("cli: copy cli: trestle up local/dev@1");
  await page.keyboard.press("Enter");
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __copied?: string }).__copied))
    .toBe("trestle up local/dev@1");
});

test("a running environment's MLflow shows in Apps, under ⌘K hides, and pops out", async ({
  page,
}) => {
  type Embed = {
    module: string;
    bounds: { x: number; y: number; width: number; height: number } | null;
  };
  const lastEmbed = () =>
    page.evaluate(() => (window as unknown as { __LL_EMBEDS__?: Embed[] }).__LL_EMBEDS__?.at(-1));
  await commitDev(page);
  await envHeader(page).getByRole("button", { name: "Start" }).click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await envHeader(page).getByRole("button", { name: "Apps" }).click();
  await appsSidebar(page).getByRole("button", { name: "MLflow" }).click();

  // Over the view's area, as laid out: what the native webview covers.
  const area = await page.getByTestId("service-embed").boundingBox();
  if (!area) throw new Error("no embed area");
  await expect.poll(lastEmbed).toEqual({
    module: "mlflow",
    bounds: {
      x: Math.round(area.x),
      y: Math.round(area.y),
      width: Math.round(area.width),
      height: Math.round(area.height),
    },
  });

  // The palette would be painted over: the embed hides while it is open.
  await page.keyboard.press("ControlOrMeta+k");
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect.poll(async () => (await lastEmbed())?.bounds ?? null).toBe(null);
  await page.keyboard.press("Escape");
  await expect.poll(async () => (await lastEmbed())?.bounds ?? null).not.toBe(null);

  await page.getByRole("button", { name: "Pop out" }).click();
  await expect
    .poll(() => page.evaluate(() => (window as { __LL_OPENED__?: string[] }).__LL_OPENED__))
    .toEqual(["mlflow"]);
  await expect(page.getByText("MLflow is open in its own window.")).toBeVisible();
  await expect.poll(async () => (await lastEmbed())?.bounds ?? null).toBe(null);
});

test("Apps lists what runs, opens its Catalog, and empties when it stops", async ({ page }) => {
  await appsActivity(page).click();
  await expect(page.getByTestId("apps-empty")).toBeVisible();
  await page.getByRole("button", { name: "Show the environments" }).click();

  await commitDev(page);
  await envHeader(page).getByRole("button", { name: "Start" }).click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await envHeader(page).getByRole("button", { name: "Apps" }).click();
  await expect(appsSidebar(page).getByRole("button", { name: /dev #1/ })).toHaveAttribute(
    "aria-current",
    "true",
  );
  await expect(page.getByTestId("apps-main-catalog")).toBeVisible();

  // Back in the editor, the tab is as it was; Stop empties the view.
  await appsActivity(page).click();
  await envHeader(page).getByRole("button", { name: "Stop" }).click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "stopped");
  await appsActivity(page).click();
  await expect(page.getByTestId("apps-empty")).toBeVisible();
});

test("an environment on Neon shows and picks its branch in its header", async ({ page }) => {
  // Unity Catalog and headwaters as Neon Functions, as the inspector's hosted choice sets.
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
    const fn = { vendor: "neon-function", connection: { kind: "function" } };
    await state.stateSetSelection(dev.id, {
      ...selection,
      modules: [...new Set([...selection.modules, "unity-catalog-rs", "headwaters"])],
      hosted: { "unity-catalog-rs": fn, headwaters: fn },
    });
  });
  await commitDev(page);

  // The header says where it would run before any Start.
  await expect(envHeader(page)).toContainText("☁ no branch yet");
  await envHeader(page).getByRole("button", { name: "☁ Pick a Neon branch" }).click();
  const dialog = page.getByTestId("neon-target");
  await expect(dialog).toContainText("as functions");
  await dialog.getByTestId("neon-target-staging").check();
  await dialog.getByTestId("neon-target-save").click();
  await expect(envHeader(page)).toContainText("☁ staging");

  // Start binds it, without asking.
  await envHeader(page).getByRole("button", { name: "Start" }).click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await expect(page.getByTestId("neon-target")).toHaveCount(0);
  await expect(envHeader(page)).toContainText("☁ staging · local slot 0");
  // Its functions serve the apps.
  await envHeader(page).getByRole("button", { name: "Apps" }).click();
  await expect(appsSidebar(page).getByRole("button", { name: "Catalog" })).toBeVisible();
  await expect(appsSidebar(page).getByRole("button", { name: "Lineage" })).toBeVisible();
  await appsActivity(page).click();
  await envHeader(page).getByRole("button", { name: "Stop" }).click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "stopped");
});

test("the Neon view deletes a branch trestle made, after confirming", async ({ page }) => {
  await openSettings(page, "neon");
  await expect(page.getByTestId("neon-card")).toContainText("cool-poetry-28343550");
  await expect(page.getByTestId("neon-staging-delete")).toHaveCount(0);
  await page.getByTestId("neon-scratch-delete").click();
  await page.getByTestId("neon-scratch-delete-confirm").click();
  await expect(page.getByTestId("neon-scratch")).toHaveCount(0);
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
  await commitDev(page);

  await openSettings(page, "workspaces");
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
  await closeSettings(page);
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
