// The Agent app end to end, in WebKit on the mocked IPC: the scripted Omnigent of
// src/host/fake-agent.ts behind the session commands, src/host/fake-uc-mcp.ts behind
// `uc-mcp` with mangrove's real grants app, and src/host/fake-workgraph-mcp.ts behind
// the factory's `/mcp` with workgraph-mcp's real plan review. A grant request opens the
// form inline; a planner's submit and plan review open the review. The app's outcome
// (`apply_grants`, `approve_revision`, `return_plan`, over the desktop's connection)
// goes back into the session as an `<mcp-app-context>` message.
import { expect, type Page, test } from "@playwright/test";
import { clickMenuAction, startAgentEnvironment } from "./helpers";

const ucCalls = (page: Page) =>
  page.evaluate(() => window.__LL_UC_MCP__?.calls.map((c) => c.name) ?? []);
const workgraphCalls = (page: Page) =>
  page.evaluate(() => window.__LL_WORKGRAPH_MCP__?.calls ?? []);
const sent = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as { __LL_AGENT__?: { sent: { text: string }[] } }).__LL_AGENT__?.sent.map(
        (s) => s.text,
      ) ?? [],
  );

test("a grant request renders mangrove's form inline, and its outcome goes back to the agent", async ({
  page,
}) => {
  await startAgentEnvironment(page);
  await clickMenuAction(page, "Apps");
  await page.getByTestId("apps-sidebar").getByRole("button", { name: "Agent" }).click();
  await expect(page.getByTestId("agent-view")).toBeVisible();

  const box = page.getByRole("textbox", { name: "Message the agent" });
  await box.fill("grant SELECT on main.default.t to group x");
  await box.press("Enter");

  // The agent's call shows, then its app, from a re-run of the tool on our connection.
  const conversation = page.getByRole("log", { name: "Conversation with the agent" });
  await expect(conversation.getByText("I'll open the grant form.")).toBeVisible();
  const frame = page.locator('[data-testid="agent-app-call_1"] [data-slot="mcp-app-frame"]');
  await expect(frame).toHaveAttribute("data-status", "ready", { timeout: 20_000 });
  await expect.poll(() => ucCalls(page)).toContain("open_grant_form");

  // The form is prefilled from the agent's arguments.
  const view = page.frameLocator('iframe[title$="interactive app"]').frameLocator("iframe");
  await expect(view.getByText("main.default.t").first()).toBeVisible({ timeout: 20_000 });

  // Submit: apply_grants through the desktop's connection, never the agent's.
  await view
    .getByRole("button", { name: /^grant/i })
    .last()
    .click();
  await expect.poll(() => ucCalls(page)).toContain("apply_grants");
  // The outcome goes into the session, tied to the agent's call.
  await expect
    .poll(async () => (await sent(page)).at(-1) ?? "")
    .toMatch(/^<mcp-app-context tool="open_grant_form" call_id="call_1">Granted /);
  await expect(page.getByTestId("agent-context-chip")).toContainText("open_grant_form: Granted");
  await expect(conversation.getByText("Done: x now holds SELECT on main.default.t.")).toBeVisible();
  // The re-run happened once: the agent's call isn't repeated by the reply.
  expect((await ucCalls(page)).filter((c) => c === "open_grant_form")).toHaveLength(1);
});

/** The Agent app on `dev`, with the plan review open after the planner's turn. */
async function openPlanReview(page: Page) {
  await startAgentEnvironment(page);
  await clickMenuAction(page, "Apps");
  await page.getByTestId("apps-sidebar").getByRole("button", { name: "Agent" }).click();
  await expect(page.getByTestId("agent-view")).toBeVisible();

  const box = page.getByRole("textbox", { name: "Message the agent" });
  await box.fill("plan the release, and send it for review");
  await box.press("Enter");

  // The planner submits (a plain tool card), then opens the review: its app, from a
  // re-run of the read-only tool on the desktop's own connection to the factory.
  const conversation = page.getByRole("log", { name: "Conversation with the agent" });
  await expect(
    conversation.getByText("I'll submit the plan and open it for your review."),
  ).toBeVisible();
  await expect(page.locator('[data-testid="agent-app-submit_1"]')).toHaveCount(0);
  const frame = page.locator('[data-testid="agent-app-call_1"] [data-slot="mcp-app-frame"]');
  await expect(frame).toHaveAttribute("data-status", "ready", { timeout: 20_000 });
  await expect
    .poll(async () => (await workgraphCalls(page)).map((c) => c.name))
    .toEqual(["open_plan_review"]);
  const view = page.frameLocator('iframe[title$="interactive app"]').frameLocator("iframe");
  await expect(view.getByRole("heading", { name: "Typed work graph on olai-store" })).toBeVisible({
    timeout: 20_000,
  });
  await expect(view.getByRole("treeitem").first()).toBeVisible();
  return { conversation, view };
}

test("a planner's plan review renders inline, and its approval goes back to the agent", async ({
  page,
}) => {
  const { conversation, view } = await openPlanReview(page);

  // Approve: approve_revision through the desktop's connection, never the agent's.
  await view.getByRole("button", { name: /^Approve revision/ }).click();
  await expect(view.getByRole("status")).toContainText("Approved");
  await expect
    .poll(async () => (await workgraphCalls(page)).map((c) => c.name))
    .toEqual(["open_plan_review", "approve_revision"]);
  const [, approval] = await workgraphCalls(page);
  expect(approval.arguments.revision).toMatch(/^[0-9a-f]{64}$/);

  // The outcome goes into the session, tied to the planner's call.
  await expect
    .poll(async () => (await sent(page)).at(-1) ?? "")
    .toMatch(/^<mcp-app-context tool="open_plan_review" call_id="call_1">Approved revision /);
  await expect(page.getByTestId("agent-context-chip")).toContainText("open_plan_review: Approved");
  await expect(
    conversation.getByText("The plan is approved: its first items can start."),
  ).toBeVisible();
  // The review ran once: the agent's reply doesn't repeat it.
  expect((await workgraphCalls(page)).filter((c) => c.name === "open_plan_review")).toHaveLength(1);
});

test("a returned plan carries the notes back to the planner", async ({ page }) => {
  const { conversation, view } = await openPlanReview(page);

  await view.getByRole("textbox", { name: "Notes for the planner" }).fill("Split the server task.");
  await view.getByRole("button", { name: "Return with notes" }).click();
  await expect(view.getByRole("status")).toContainText("Returned");
  await expect
    .poll(async () => (await workgraphCalls(page)).at(-1)?.arguments.notes)
    .toBe("Split the server task.");
  await expect
    .poll(async () => (await sent(page)).at(-1) ?? "")
    .toMatch(
      /^<mcp-app-context tool="open_plan_review" call_id="call_1">Returned [\s\S]*Split the server task\./,
    );
  await expect(
    conversation.getByText("I'll revise the plan with your notes and submit it again."),
  ).toBeVisible();
});
