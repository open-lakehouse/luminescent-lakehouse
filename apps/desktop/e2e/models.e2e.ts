// The Models choice end to end, in WebKit on the mocked IPC (src/host/fake-models.ts):
// the first Start of an environment with MLflow asks where its models come from, a
// Databricks profile of the CLI is picked, and the environment starts; a secret its
// revision requires and nothing sets is named before a Start. The Neon AI Gateway is
// offered on a branch only, lists the branch's models, and shows its credit, plan and
// region problems on the option.
import { expect, type Page, test } from "@playwright/test";

const subTabs = (page: Page) => page.getByRole("tablist", { name: "Environment views" });
const envHeader = (page: Page) => subTabs(page).locator("xpath=preceding-sibling::div[1]");
const envState = (page: Page) => envHeader(page).locator("[data-run-state]").first();

/** Add MLflow to dev's draft (as the CLI would), and a Neon Function with `onNeon`,
 *  then commit it in History. */
async function commitWithMlflow(page: Page, { onNeon = false } = {}) {
  await page.goto("/");
  // The starter workspace and its dev, once the app made them.
  await expect(
    page.getByTestId("workspace-view").getByText("dev", { exact: true }).first(),
  ).toBeVisible();
  await page.evaluate(async (onNeon) => {
    const state = (
      window as unknown as {
        __LL_FAKE_STATE__: {
          stateWorkspaces(): Promise<{ data: { id: string }[] }>;
          stateWorkspace(id: string): Promise<{
            data: { environments: { id: string; selection: { modules: string[] } }[] };
          }>;
          stateSetSelection(id: string, selection: unknown): Promise<unknown>;
        };
      }
    ).__LL_FAKE_STATE__;
    const [ws] = (await state.stateWorkspaces()).data;
    const [env] = (await state.stateWorkspace(ws?.id ?? "")).data.environments;
    if (!env) throw new Error("no environment");
    const fn = { vendor: "neon-function", connection: { kind: "function" } };
    await state.stateSetSelection(env.id, {
      ...env.selection,
      modules: [...env.selection.modules, "mlflow", ...(onNeon ? ["unity-catalog-rs"] : [])],
      ...(onNeon ? { hosted: { "unity-catalog-rs": fn } } : {}),
    });
  }, onNeon);
  await page.getByRole("button", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit" }).click();
  await expect(page.getByRole("list", { name: "Revisions of dev" })).toContainText("#1");
}

test("the first Start asks for the models, and a Databricks profile starts it", async ({
  page,
}) => {
  await commitWithMlflow(page);
  await envHeader(page).getByRole("button", { name: "Start" }).click();
  const dialog = page.getByTestId("models-dialog");
  await expect(dialog).toContainText("Models for dev");
  await expect(page.getByTestId("models-neon")).toBeDisabled();
  await page.getByTestId("models-databricks").check();
  await expect(page.getByTestId("models-profile")).toHaveValue("work");
  await page.getByTestId("models-save").click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
  const choice = await page.evaluate(() =>
    (
      window as unknown as { __LL_MODELS__: { choices: Map<string, unknown> } }
    ).__LL_MODELS__.choices
      .values()
      .next(),
  );
  expect(choice.value).toEqual({
    kind: "databricksProfile",
    profile: "work",
    host: "https://dbc-1.cloud.databricks.com",
  });
  await expect(envHeader(page).getByRole("button", { name: "Databricks · work ▾" })).toBeDisabled();
});

test("a profile with no host is named as a missing secret before the Start, and picking it again fixes it", async ({
  page,
}) => {
  await commitWithMlflow(page);
  // As a hand-made variable set would leave it: a profile, no workspace URL.
  await page.evaluate(async () => {
    const models = window as unknown as {
      __LL_MODELS__: { set(env: string, choice: unknown): Promise<unknown> };
      __LL_FAKE_STATE__: {
        stateWorkspaces(): Promise<{ data: { id: string }[] }>;
        stateWorkspace(id: string): Promise<{ data: { environments: { id: string }[] } }>;
      };
    };
    const state = models.__LL_FAKE_STATE__;
    const [ws] = (await state.stateWorkspaces()).data;
    const [env] = (await state.stateWorkspace(ws?.id ?? "")).data.environments;
    await models.__LL_MODELS__.set(env?.id ?? "", {
      kind: "databricksProfile",
      profile: "work",
      host: "",
    });
  });
  await envHeader(page).getByRole("button", { name: "Start" }).click();
  await expect(page.getByTestId("models-missing")).toContainText("DATABRICKS_HOST");
  // Saving the CLI's profile writes its host too: nothing is missing, and it starts.
  await expect(page.getByTestId("models-profile")).toHaveValue("work");
  await page.getByTestId("models-save").click();
  await expect(envState(page)).toHaveAttribute("data-run-state", "running");
});

/** dev with MLflow on branch `staging` (its header's picker), the gateway in `scenario`. */
async function onStaging(page: Page, scenario: string) {
  await commitWithMlflow(page, { onNeon: true });
  await page.evaluate((scenario) => {
    (
      window as unknown as { __LL_MODELS__: { setGateway(s: string): void } }
    ).__LL_MODELS__.setGateway(scenario);
  }, scenario);
  await envHeader(page).getByRole("button", { name: "Pick a Neon branch ▾" }).click();
  await page.getByTestId("neon-target-staging").check();
  await page.getByTestId("neon-target-save").click();
  await expect(envHeader(page).getByRole("button", { name: "staging ▾" })).toBeVisible();
  await envHeader(page).getByRole("button", { name: "Choose models ▾" }).click();
  await page.getByTestId("models-neon").check();
}

test("on a branch, the Neon AI Gateway lists its models and is chosen", async ({ page }) => {
  await onStaging(page, "ok");
  const models = page.getByTestId("models-neon-models");
  await expect(models).toContainText("3 models on branch staging");
  await expect(models).toContainText("claude-opus-5-5");
  await expect(page.getByTestId("models-neon-missing")).toContainText("claude-sonnet-5-5");
  await page.getByTestId("models-save").click();
  await expect(page.getByTestId("models-dialog")).toBeHidden();
  await expect(envHeader(page).getByRole("button", { name: "Neon AI Gateway ▾" })).toBeVisible();
});

test("the gateway's credit, plan and region problems show on the option", async ({ page }) => {
  await onStaging(page, "credits");
  const problem = page.getByTestId("models-neon-problem");
  await expect(problem).toHaveAttribute("data-problem", "credits");
  await expect(problem).toContainText("no Neon AI Gateway credits");
  await expect(page.getByTestId("models-neon-console")).toHaveText(
    "Add credits in the Neon Console",
  );
  await expect(page.getByTestId("models-save")).toBeEnabled();

  // Asked again on a free plan, then in a region without the gateway: no Save.
  for (const [scenario, text] of [
    ["paidPlan", "needs a paid Neon plan"],
    ["region", "has no Neon AI Gateway"],
  ]) {
    await page.evaluate(
      (scenario) =>
        (
          window as unknown as { __LL_MODELS__: { setGateway(s: string): void } }
        ).__LL_MODELS__.setGateway(scenario),
      scenario,
    );
    await page.getByTestId("models-neon-recheck").click();
    await expect(page.getByTestId("models-neon-problem")).toContainText(text);
    await expect(page.getByTestId("models-save")).toBeDisabled();
  }
});

test("without a branch the option is disabled, with the reason", async ({ page }) => {
  await commitWithMlflow(page);
  await envHeader(page).getByRole("button", { name: "Choose models ▾" }).click();
  await expect(page.getByTestId("models-neon")).toBeDisabled();
  await expect(page.getByTestId("models-neon-reason")).toContainText("Pick a Neon branch");
});
