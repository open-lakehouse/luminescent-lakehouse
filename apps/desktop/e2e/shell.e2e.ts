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

  await page.getByRole("button", { name: "dev containers" }).click();
  await expect(page.getByTestId("run-services")).toContainText("unity-catalog");

  await page.getByTestId("run-dev-stop").click();
  await expect(state).toHaveText("stopped");
});
