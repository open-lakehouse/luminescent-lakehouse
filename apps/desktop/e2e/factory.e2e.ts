// The Factory app end to end, in WebKit on the mocked IPC: the control room on the
// shell's `factory` binding, which src/host/ipc-mock.ts answers with factory-client's
// in-memory factory seeded with the reference demo plan (as the in-process factory is
// seeded with factory-core's), and "Open session" on the scripted Omnigent of
// src/host/fake-agent.ts, which has the demo's sessions.
import { expect, type Page, test } from "@playwright/test";
import { clickMenuAction, startAgentEnvironment } from "./helpers";

const controlRoom = (page: Page) => page.getByTestId("factory-control-room");
const runSection = (page: Page) => controlRoom(page).getByRole("region", { name: "Run" });

/** Select the demo's node that has a run (the backend task, in review). Dispatched,
 *  not clicked at a point: the canvas sometimes fits the graph before its pane has its
 *  width, leaving the node under the inspector, which then takes a real click. */
async function selectRunNode(page: Page) {
  await controlRoom(page).getByTestId("run").dispatchEvent("click");
  await expect(runSection(page)).toContainText("sess-demo-2");
}

test("the control room loads the reference demo plan, with nothing running", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("shell")).toBeVisible();
  await page.getByLabel("Views").getByRole("button", { name: "Apps" }).click();
  await expect(page.getByTestId("apps-empty")).toBeVisible();
  await page.getByTestId("apps-sidebar").getByRole("button", { name: "Factory" }).click();

  await expect(controlRoom(page)).toBeVisible();
  await expect(controlRoom(page).getByRole("list", { name: "Plans" })).toContainText(
    "Typed work graph on olai-store",
  );
  await expect(controlRoom(page).getByRole("heading", { level: 2 })).toHaveText(
    "Typed work graph on olai-store",
  );
  await expect(controlRoom(page).getByTestId("revision-timeline")).toContainText("approved");
  expect(await controlRoom(page).locator("[data-testid^='work-node-']").count()).toBeGreaterThan(3);

  await selectRunNode(page);
  await expect(runSection(page)).toContainText("factory-reviewer");
  // No environment runs Omnigent: a session has nowhere to open.
  await runSection(page)
    .getByRole("button", { name: /^Open session/ })
    .last()
    .click();
  await expect(page.getByTestId("factory-no-omnigent")).toBeVisible();
});

test("Open session shows the run's session in the Agent view, and goes back", async ({ page }) => {
  await startAgentEnvironment(page);
  await clickMenuAction(page, "Apps");
  await page.getByTestId("apps-sidebar").getByRole("button", { name: "Factory" }).click();
  await selectRunNode(page);
  await runSection(page)
    .getByRole("button", { name: /^Open session/ })
    .last()
    .click();

  const view = page.getByTestId("agent-view");
  await expect(view).toBeVisible();
  await expect(view).toContainText("factory-reviewer");
  await expect(view).toContainText("Reviewed: one nit on the conformance suite's naming.");
  await expect(view.getByRole("button", { name: /New chat/ })).toHaveCount(0);

  await page.getByRole("button", { name: /Control room/ }).click();
  await expect(controlRoom(page)).toBeVisible();
  await expect(runSection(page)).toContainText("sess-demo-2");
});
