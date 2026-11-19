// The Agent app end to end, in WebKit on the mocked IPC: the scripted Omnigent of
// src/host/fake-agent.ts behind the session commands, and src/host/fake-uc-mcp.ts behind
// `uc-mcp` with mangrove's real grants app. A grant request opens the form inline; its
// submit calls `apply_grants` over the desktop's connection, and the outcome goes back
// into the session as an `<mcp-app-context>` message.
import { expect, type Page, test } from "@playwright/test";
import { envHeader, startAgentEnvironment } from "./helpers";

const ucCalls = (page: Page) =>
  page.evaluate(() => window.__LL_UC_MCP__?.calls.map((c) => c.name) ?? []);
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
  await envHeader(page).getByRole("button", { name: "Apps" }).click();
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
