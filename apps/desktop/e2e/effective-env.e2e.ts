// The effective-env view end to end, in WebKit on the mocked IPC (src/host/fake-vars.ts):
// after a Start, the bottom panel's Environment lists each variable with its layer (this
// machine's local vars, an instance override shadowing the environment's), and the copy
// is a masked dotenv.
import { expect, type Page, test } from "@playwright/test";
import { envState, runAction } from "./helpers";

type Vars = {
  set(target: unknown, key: string, input: unknown): Promise<unknown>;
};

/** Every `navigator.clipboard.writeText` lands in `window.__copied`. */
async function captureClipboard(page: Page) {
  await page.addInitScript(() => {
    const w = window as unknown as { __copied: string[] };
    w.__copied = [];
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: async (text: string) => void w.__copied.push(text) },
    });
  });
}

const row = (page: Page, key: string) =>
  page.locator(`[data-testid="effective-row"][data-key="${key}"]`);

test("the Environment panel shows each variable's layer, and copies a masked dotenv", async ({
  page,
}) => {
  await captureClipboard(page);
  await page.goto("/");
  await page.getByRole("tab", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit", exact: true }).click();
  await expect(page.getByRole("region", { name: "Revisions of dev" })).toContainText("#1");
  await runAction(page, "Start").click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");

  // Variables as the editors would set them: the environment's, this machine's
  // secret, and the instance's override of the environment's.
  await page.evaluate(async () => {
    const w = window as unknown as {
      __LL_VARS__: Vars;
      __LL_FAKE_STATE__: {
        stateWorkspaces(): Promise<{ data: { id: string }[] }>;
        runWorkspace(id: string): Promise<{
          data: { environments: { id: string; instances: { id: string }[] }[] };
        }>;
      };
    };
    const [ws] = (await w.__LL_FAKE_STATE__.stateWorkspaces()).data;
    const [env] = (await w.__LL_FAKE_STATE__.runWorkspace(ws?.id ?? "")).data.environments;
    const instance = env?.instances[0]?.id ?? "";
    const literal = (value: string) => ({ kind: "literal", value });
    await w.__LL_VARS__.set(
      { scope: "environment", environment: env?.id },
      "LEVEL",
      literal("info"),
    );
    await w.__LL_VARS__.set({ scope: "instance", instance }, "LEVEL", literal("debug"));
    await w.__LL_VARS__.set({ scope: "local", environment: env?.id }, "TOKEN", {
      kind: "secret",
      value: "e2e-s3cret",
    });
  });

  await page
    .getByRole("region", { name: "Panel" })
    .getByRole("tab", { name: "Environment" })
    .click();
  await page.getByTestId("effective-refresh").click();
  const level = row(page, "LEVEL");
  await expect(level).toHaveAttribute("data-layer", "instance overrides");
  await expect(level).toContainText("debug");
  await expect(level.getByTestId("effective-shadowed")).toHaveText("over environment vars");
  const token = row(page, "TOKEN");
  await expect(token).toHaveAttribute("data-layer", "local vars");
  await expect(token).toContainText("••••••");
  await expect(page.getByTestId("effective-panel")).not.toContainText("e2e-s3cret");
  await expect(row(page, "TRESTLE_INSTANCE")).toHaveAttribute("data-layer", "placement");

  await page.getByTestId("effective-copy").click();
  const copied = await page.evaluate(() => (window as unknown as { __copied: string[] }).__copied);
  expect(copied).toHaveLength(1);
  expect(copied[0]).toMatch(/^# local\/dev@1 \(.+\), from the host; secrets masked\n/);
  expect(copied[0]).toContain("# instance overrides\nLEVEL=debug\n");
  expect(copied[0]).toContain("# local vars\nTOKEN=••••••\n");
  expect(copied[0]).not.toContain("e2e-s3cret");
});

test("a long value keeps the Environment panel's table to the panel's width", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("tab", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit", exact: true }).click();
  await expect(page.getByRole("region", { name: "Revisions of dev" })).toContainText("#1");
  await runAction(page, "Start").click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await page.evaluate(async () => {
    const w = window as unknown as {
      __LL_VARS__: Vars;
      __LL_FAKE_STATE__: {
        stateWorkspaces(): Promise<{ data: { id: string }[] }>;
        runWorkspace(id: string): Promise<{ data: { environments: { id: string }[] } }>;
      };
    };
    const [ws] = (await w.__LL_FAKE_STATE__.stateWorkspaces()).data;
    const [env] = (await w.__LL_FAKE_STATE__.runWorkspace(ws?.id ?? "")).data.environments;
    await w.__LL_VARS__.set({ scope: "local", environment: env?.id }, "LONG_URL", {
      kind: "literal",
      value: `https://${"a".repeat(400)}.example.com`,
    });
  });
  await page
    .getByRole("region", { name: "Panel" })
    .getByRole("tab", { name: "Environment" })
    .click();
  await page.getByTestId("effective-refresh").click();
  await expect(row(page, "LONG_URL")).toBeVisible();
  const widths = await page.getByTestId("effective-panel").evaluate((panel) => {
    const table = panel.querySelector('[data-testid="effective-table"]') as HTMLElement;
    return { panel: panel.clientWidth, table: table.getBoundingClientRect().width };
  });
  expect(widths.table).toBeLessThanOrEqual(widths.panel);
});
