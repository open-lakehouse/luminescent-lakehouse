// What the specs share: an environment's header, and `dev` started with Omnigent and
// uc-mcp among its services (the fake state's).
import { expect, type Page } from "@playwright/test";

export const subTabs = (page: Page) => page.getByRole("tablist", { name: "Environment views" });
export const envHeader = (page: Page) => subTabs(page).locator("xpath=preceding-sibling::div[1]");
export const envState = (page: Page) => envHeader(page).locator("[data-run-state]").first();

/** Start `dev` with Omnigent and uc-mcp running too (the fake state's services). */
export async function startAgentEnvironment(page: Page) {
  await page.goto("/");
  await page.getByRole("button", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit" }).click();
  await expect(page.getByRole("list", { name: "Revisions of dev" })).toContainText("#1");
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
