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

test("commits a revision of the draft (in memory)", async ({ page }) => {
  await page.getByRole("button", { name: "History" }).click();
  await expect(page.getByText("No revisions yet.")).toBeVisible();
  await page.getByLabel("Revision message").fill("first cut");
  await page.getByRole("button", { name: "Commit" }).click();
  const revisions = page.getByRole("list", { name: "Revisions of dev" });
  await expect(revisions).toContainText("first cut");
  await expect(revisions).toContainText("#1");
  await expect(page.getByText("same as #1")).toBeVisible();
});
