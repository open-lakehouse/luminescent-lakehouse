// The shell in WebKit, on the mocked IPC (src/host/ipc-mock.ts): the one frame
// (env-editor's workspace shell with the desktop's views), the doctor, each
// environment's run controls and sub-tabs, and the Apps view.
import { expect, type Page, test } from "@playwright/test";
import {
  cardDetail,
  clickCardDetail,
  clickMenuAction,
  envCard,
  envState,
  runAction,
  subTabs,
} from "./helpers";

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

async function commitDev(page: Page) {
  await page.getByRole("tab", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit", exact: true }).click();
  await expect(page.getByRole("region", { name: "Revisions of dev" })).toContainText("#1");
}

/** The Apps view's item in the activity bar. */
const appsActivity = (page: Page) =>
  page.getByRole("navigation", { name: "Views" }).getByRole("button", { name: "Apps" });

/** The Apps view's sidebar: the running instances and the selected one's apps. */
const appsSidebar = (page: Page) => page.getByTestId("apps-sidebar");

/** The bottom panel's tab `name` (Problems, Run, Logs). */
const panelTab = (page: Page, name: string) =>
  page.getByRole("region", { name: "Panel" }).getByRole("tab", { name, exact: true });

/** Show the bottom panel's Run view. */
const openRunPanel = (page: Page) => panelTab(page, "Run").click();

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

test("one activity bar: the editor, Apps and the gear; the editor's views in the sidebar, no Modules", async ({
  page,
}) => {
  const views = page.getByRole("navigation", { name: "Views" });
  await expect(views).toHaveCount(1);
  const labels = await views
    .getByRole("button")
    .evaluateAll((bs) => bs.map((b) => b.getAttribute("aria-label")));
  expect(labels).toEqual(["Editor", "Apps", "Settings"]);
  const tabs = await page
    .getByRole("tablist", { name: "Sidebar views" })
    .getByRole("tab")
    .evaluateAll((ts) => ts.map((t) => t.getAttribute("aria-label")));
  expect(tabs).toEqual(["Environments", "History", "Workspace settings"]);
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
  await page.getByRole("tab", { name: "History" }).click();
  await expect(page.getByText("No revisions yet.")).toBeVisible();
  await page.getByLabel("Revision message").fill("first cut");
  await page.getByRole("button", { name: "Commit", exact: true }).click();
  // The revision is numbered, and History shows its message.
  const revisions = page.getByRole("region", { name: "Revisions of dev" });
  await expect(revisions).toContainText("#1");
  await expect(revisions).toContainText("first cut");
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

test("the tree row starts it, and resting on it longer shows its card", async ({ page }) => {
  await commitDev(page);
  await page.getByRole("tab", { name: "Environments" }).click();
  const row = page.getByRole("treeitem").filter({ hasText: "dev" });
  await row.hover();
  await row.getByRole("button", { name: "Start", exact: true }).click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await row.hover();
  await expect(page.locator('[aria-label="About dev"]')).toContainText("local slot 0");
  // Rename, Duplicate and Remove are in its menu.
  await row.getByRole("button", { name: "More actions for dev" }).click();
  await expect(page.getByRole("menuitem", { name: "Rename" })).toBeVisible();
  await page.keyboard.press("Escape");
  await row.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "stopped");
});

test("Commit and start commits the draft, then starts the new head", async ({ page }) => {
  await page.getByRole("tab", { name: "History" }).click();
  await page.getByRole("button", { name: "More ways to commit" }).click();
  await page.getByRole("menuitem", { name: "Commit and start" }).click();
  await expect(page.getByRole("region", { name: "Revisions of dev" })).toContainText("#1");
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await expect(await envCard(page)).toContainText("rev 1");
});

test("Start runs the committed revision, Stop takes it down", async ({ page }) => {
  await commitDev(page);
  await expect(envState(page)).toHaveAttribute("data-run-state", "stopped");
  await runAction(page, "Start").click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await expect(await envCard(page)).toContainText("rev 1 · local slot 0");

  // The gateway's URL is a card detail, copyable; in the Run panel a link to its apps.
  await expect(await cardDetail(page, "Gateway")).toContainText("http://localhost:9080");
  await expect((await envCard(page)).getByRole("button", { name: "Copy Gateway" })).toBeVisible();
  await openRunPanel(page);
  await expect(page.getByTestId("gateway-url")).toContainText("http://localhost:9080");
  await expect(page.getByTestId("run-services")).toContainText("unitycatalog");

  // Logs is the bottom panel's, next to Run.
  await panelTab(page, "Logs").click();
  await expect(page.getByTestId("logs-lines")).toBeVisible();
  await openRunPanel(page);
  await page.getByTestId("gateway-url").click();
  await expect(appsSidebar(page).getByRole("button", { name: "Catalog" })).toBeVisible();
  await appsActivity(page).click();

  await runAction(page, "Stop").click();
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

test("MLflow's Connect an agent shows the snippets over a hidden embed, and copies them", async ({
  page,
}) => {
  type Embed = { module: string; bounds: unknown };
  const lastBounds = () =>
    page.evaluate(
      () =>
        (window as unknown as { __LL_EMBEDS__?: Embed[] }).__LL_EMBEDS__?.at(-1)?.bounds ?? null,
    );
  await commitDev(page);
  await runAction(page, "Start").click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await clickMenuAction(page, "Apps");
  await appsSidebar(page).getByRole("button", { name: "MLflow" }).click();
  await expect.poll(lastBounds).not.toBe(null);

  await page.getByTestId("connect-agent-open").click();
  const snippet = page.getByTestId("connect-agent-snippet");
  await expect(snippet).toContainText(
    'export ANTHROPIC_BASE_URL="http://localhost:9080/ai-gateway/anthropic"',
  );
  await expect(snippet).toContainText("Databricks profile work");
  // The dialog would be painted over: the page hides while it is open.
  await expect.poll(lastBounds).toBe(null);
  await page.getByTestId("connect-agent-codex").click();
  await expect(snippet).toContainText('base_url = "http://localhost:9080/ai-gateway/openai/v1"');
  await page.evaluate(() => {
    const copied: string[] = [];
    (window as unknown as { __COPIED__: string[] }).__COPIED__ = copied;
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: async (text: string) => void copied.push(text) },
    });
  });
  await page.getByTestId("connect-agent-copy").click();
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __COPIED__: string[] }).__COPIED__))
    .toEqual([expect.stringContaining("[model_providers.lakehouse]")]);
  await page.keyboard.press("Escape");
  await expect.poll(lastBounds).not.toBe(null);
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
  await runAction(page, "Start").click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await clickMenuAction(page, "Apps");
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
  await runAction(page, "Start").click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await clickMenuAction(page, "Apps");
  await expect(appsSidebar(page).getByRole("button", { name: /dev #1/ })).toHaveAttribute(
    "aria-current",
    "true",
  );
  await expect(page.getByTestId("apps-main-catalog")).toBeVisible();

  // Back in the editor, the tab is as it was; Stop empties the view.
  await appsActivity(page).click();
  await runAction(page, "Stop").click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "stopped");
  await appsActivity(page).click();
  await expect(page.getByTestId("apps-empty")).toBeVisible();
});

test("Query runs SQL on the instance and shows its rows, or why it failed", async ({ page }) => {
  await commitDev(page);
  await runAction(page, "Start").click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await clickMenuAction(page, "Apps");
  await appsSidebar(page).getByRole("button", { name: "Query" }).click();
  const view = page.getByTestId("query-view");
  await expect(view).toBeVisible();
  // The starter query is in Monaco, with its SQL highlighting.
  await expect(view.locator(".monaco-editor .view-lines")).toContainText("SELECT 1 AS one");

  await view.getByRole("button", { name: "Run" }).click();
  const results = page.getByTestId("query-results");
  await expect(results).toContainText("grace");
  await expect(page.getByTestId("query-status")).toContainText("3 rows");

  // Cmd/Ctrl+Enter runs what is typed; a table the catalog lacks says so.
  // Monaco takes Cmd or Ctrl from the page's user agent, which Desktop Safari
  // makes a Mac's on every host, so `ControlOrMeta` would send Ctrl on Linux.
  const isMac = await page.evaluate(() => /Macintosh|Mac OS X/.test(navigator.userAgent));
  const mod = isMac ? "Meta" : "Control";
  await view.locator(".monaco-editor .view-lines").click();
  await page.keyboard.press(`${mod}+A`);
  await page.keyboard.type("SELECT * FROM demo.raw.nope");
  await page.keyboard.press(`${mod}+Enter`);
  await expect(results.getByRole("alert")).toContainText("table 'nope' not found");

  // The text stays with the instance across a visit to another app.
  await appsSidebar(page).getByRole("button", { name: "Catalog" }).click();
  await expect(page.getByTestId("apps-main-catalog")).toBeVisible();
  await appsSidebar(page).getByRole("button", { name: "Query" }).click();
  await expect(page.getByTestId("query-view").locator(".monaco-editor .view-lines")).toContainText(
    "demo.raw.nope",
  );
});

test("an environment on Neon shows its branch in its header, and picks it in Configure", async ({
  page,
}) => {
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

  // The card says where it would run before any Start; its detail opens Configure.
  await expect(await cardDetail(page, "Neon")).toContainText("pick a branch");
  await clickCardDetail(page, "Neon");
  const section = page.getByTestId("configure-neon");
  await expect(section).toContainText("as functions");
  await section.getByTestId("neon-target-staging").check();
  await section.getByTestId("neon-target-save").click();
  await expect(await cardDetail(page, "Neon")).toContainText("staging");

  // Start binds it, without asking.
  await runAction(page, "Start").click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await expect(page.getByTestId("configure-waiting")).toHaveCount(0);
  await expect(await cardDetail(page, "Neon")).toContainText("staging");
  await expect(await envCard(page)).toContainText("local slot 0");
  // Its functions serve the apps.
  await clickMenuAction(page, "Apps");
  await expect(appsSidebar(page).getByRole("button", { name: "Catalog" })).toBeVisible();
  await expect(appsSidebar(page).getByRole("button", { name: "Lineage" })).toBeVisible();
  await appsActivity(page).click();
  await runAction(page, "Stop").click();
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
  await page.getByRole("tab", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit", exact: true }).click();
  const revisions = page.getByRole("region", { name: "Revisions of dev" });
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
      modules: [...selection.modules, "headwaters"],
    });
  });
  await expect(page.getByRole("button", { name: "Diff" })).toBeEnabled();
  await page.getByRole("button", { name: "Diff" }).click();
  await expect(page.getByRole("list", { name: "Changes" })).toContainText("module headwaters");

  await page.getByRole("button", { name: "Commit", exact: true }).click();
  await expect(revisions).toContainText("#2");
  // The compare button shows on hover.
  await revisions.getByRole("listitem").filter({ hasText: "#2" }).hover();
  await page.getByRole("button", { name: "Compare #1 with #2" }).click();
  await expect(page.getByText("#1 → #2").first()).toBeVisible();
  await expect(page.getByRole("list", { name: "Changes" })).toContainText("module headwaters");
});

test("Files and Changes show in Monaco, read-only", async ({ page }) => {
  await commitDev(page);
  await subTabs(page).getByRole("tab", { name: "Files" }).click();
  const file = page.getByTestId("monaco-file");
  await expect(file.locator(".monaco-editor .view-lines")).toContainText(
    "Top-level compose entrypoint",
  );
  // Read-only: typing changes nothing.
  await file.locator(".monaco-editor .view-lines").click();
  await page.keyboard.type("zzz");
  await expect(file.locator(".monaco-editor .view-lines")).not.toContainText("zzz");

  // Edit the draft (a module more, as the diagram's palette would), then Changes.
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
      modules: [...selection.modules, "headwaters"],
    });
  });
  await subTabs(page).getByRole("tab", { name: "Changes" }).click();
  const diff = page.getByTestId("monaco-diff").first();
  await expect(diff.locator(".monaco-diff-editor")).toBeVisible();
  await expect(diff.locator(".line-insert, .char-insert").first()).toBeVisible();
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

test("renames an environment from its row's menu", async ({ page }) => {
  const tree = page.getByRole("tree", { name: "Environments" });
  const dev = tree.getByRole("treeitem").filter({ hasText: "dev" });
  await dev.hover();
  await dev.getByRole("button", { name: "More actions for dev" }).click();
  await page.getByRole("menuitem", { name: "Rename" }).click();
  // The name field has the focus, not the menu's button.
  const name = page.getByLabel("Environment name");
  await expect(name).toBeFocused();
  await name.fill("staging");
  await name.press("Enter");
  await expect(tree.getByRole("treeitem").filter({ hasText: "staging" })).toBeVisible();
});

test("removes an environment from the explorer after confirming", async ({ page }) => {
  const tree = page.getByRole("tree", { name: "Environments" });
  await page.getByRole("button", { name: "New environment" }).click();
  await page.getByLabel("Environment name").fill("prod");
  await page.getByLabel("Environment name").press("Enter");
  const prod = tree.getByRole("treeitem").filter({ hasText: "prod" });
  await expect(prod).toBeVisible();
  // Remove is in the row's ⋯ menu.
  const removeFromMenu = async () => {
    await prod.hover();
    await prod.getByRole("button", { name: "More actions for prod" }).click();
    await page.getByRole("menuitem", { name: "Remove" }).click();
  };

  // Cancel keeps it.
  await removeFromMenu();
  const confirm = tree.getByRole("alertdialog", { name: "Remove" });
  await expect(confirm).toContainText("Remove prod from the workspace?");
  await confirm.getByRole("button", { name: "Cancel" }).click();
  await expect(confirm).toBeHidden();
  await expect(prod).toBeVisible();

  // So does Escape.
  await removeFromMenu();
  await expect(confirm).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(confirm).toBeHidden();
  await expect(prod).toBeVisible();

  // Confirming removes it, from the shared state too.
  await removeFromMenu();
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
