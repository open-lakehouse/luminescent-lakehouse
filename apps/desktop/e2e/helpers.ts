// What the specs share: an environment's header and its Configure tab, and `dev` started with Omnigent and
// uc-mcp among its services (the fake state's).
import { expect, type Page } from "@playwright/test";

export const subTabs = (page: Page) => page.getByRole("tablist", { name: "Environment views" });
export const envHeader = (page: Page) => subTabs(page).locator("xpath=preceding-sibling::div[1]");
export const envState = (page: Page) => envHeader(page).locator("[data-run-state]").first();
/** The header's detail drawn with lucide icon `icon` (`cloud`: the Neon branch, `bot`: the
 *  models): a button showing the current choice, which opens Configure on it. */
export const headerDetail = (page: Page, icon: "cloud" | "bot") =>
  envHeader(page).locator(`button:has(svg.lucide-${icon})`);
/** The Configure tab's section `section`, opened from the sub-tabs and its nav. */
export async function openConfigure(page: Page, section: "neon" | "models" | "variables") {
  await subTabs(page).getByRole("tab", { name: "Configure" }).click();
  await page.getByTestId(`configure-nav-${section}`).click();
  return page.getByTestId(`configure-${section}`);
}

/** Start `dev` with Omnigent and uc-mcp running too (the fake state's services). */
export async function startAgentEnvironment(page: Page) {
  await page.goto("/");
  await page.getByRole("tab", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit" }).click();
  await expect(page.getByRole("region", { name: "Revisions of dev" })).toContainText("#1");
  await envHeader(page).getByRole("button", { name: "Start" }).click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  await page.evaluate(async () => {
    type Row = { service: string; module: string };
    const state = (
      window as unknown as {
        __LL_FAKE_STATE__: {
          stateWorkspaces(): Promise<{ data: { id: string }[] }>;
          runWorkspace(id: string): Promise<{
            data: { environments: { instances: { id: string; services: Row[] }[] }[] };
          }>;
          setServices(instance: string, services: Row[]): void;
        };
      }
    ).__LL_FAKE_STATE__;
    const [ws] = (await state.stateWorkspaces()).data;
    const run = (await state.runWorkspace(ws?.id ?? "")).data;
    const instance = run.environments.flatMap((e) => e.instances)[0];
    if (!instance) throw new Error("no instance");
    const row = (service: string, module: string) => ({
      ...instance.services[0],
      service,
      module,
    });
    state.setServices(instance.id, [
      ...instance.services,
      row("omnigent", "omnigent"),
      row("omnigent-host", "omnigent"),
      row("uc-mcp", "uc-mcp"),
    ]);
  });
}
