// What the specs share: an environment's editor tab, hover card, run action and ⋯ menu,
// its Configure tab, and `dev` started with Omnigent and uc-mcp among its services (the
// fake state's).
import { expect, type Locator, type Page } from "@playwright/test";

export const subTabs = (page: Page) => page.getByRole("tablist", { name: "Environment views" });
/** The row the sub-tabs end: the run action and the ⋯ menu. */
const subTabRow = (page: Page) => subTabs(page).locator("xpath=..");
/** The open environment's editor tab. */
export const envTab = (page: Page) =>
  page.getByRole("tablist", { name: "Open editors" }).getByRole("tab", { selected: true });
/** The open environment's state, as its editor tab's dot says. */
export const envState = (page: Page) => envTab(page).locator("[data-run-state]");
/** The open environment's run action (Start, Stop, …) at the end of its sub-tabs, when
 *  it is `name`. */
export const runAction = (page: Page, name: string) =>
  subTabRow(page).getByRole("button", { name, exact: true });
/** Run the entry `name` of the ⋯ menu ending the sub-tabs. */
export async function clickMenuAction(page: Page, name: string) {
  await subTabRow(page)
    .getByRole("button", { name: /^More actions for / })
    .click();
  await page.getByRole("menuitem", { name, exact: true }).click();
}
/** The open environment's hover card, shown by resting on its editor tab. */
export async function envCard(page: Page): Promise<Locator> {
  await envTab(page).hover();
  const card = page.locator('[aria-label^="About "]');
  await expect(card).toBeVisible();
  return card;
}
/** The card's detail `label` (`Neon`, `Models`, `Gateway`): its value. */
export async function cardDetail(page: Page, label: string): Promise<Locator> {
  return (await envCard(page))
    .locator("dt")
    .filter({ hasText: new RegExp(`^${label}$`) })
    .locator("xpath=following-sibling::dd[1]");
}
/** Click the card's detail `label` (`Neon`, `Models`): it opens Configure there. */
export async function clickCardDetail(page: Page, label: "Neon" | "Models") {
  await (await cardDetail(page, label)).getByRole("button").first().click();
}
/** Commit the draft from History. */
export async function commitDraft(page: Page) {
  await page.getByRole("tab", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit", exact: true }).click();
}
/** The Configure tab's section `section`, opened from the sub-tabs and its nav. */
export async function openConfigure(page: Page, section: "neon" | "models" | "variables") {
  await subTabs(page).getByRole("tab", { name: "Configure" }).click();
  await page.getByTestId(`configure-nav-${section}`).click();
  return page.getByTestId(`configure-${section}`);
}

/** Start `dev` with Omnigent and uc-mcp running too (the fake state's services). */
export async function startAgentEnvironment(page: Page) {
  await page.goto("/");
  await commitDraft(page);
  await expect(page.getByRole("region", { name: "Revisions of dev" })).toContainText("#1");
  await runAction(page, "Start").click();
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
