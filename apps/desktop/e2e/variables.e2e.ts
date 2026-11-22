// The variable editors end to end, in WebKit on the mocked IPC (src/host/fake-vars.ts):
// a literal and a keychain secret on the environment and on this machine, the secret's
// value in the keychain only (never in the state), the head revision's vars read-only,
// a failed write shown as a toast, and a provider key set from the Models dialog.
import { expect, type Page, test } from "@playwright/test";
import { envHeader } from "./helpers";

type Vars = {
  keychain: Map<string, string>;
  failNextWrite(message: string): void;
  get(environment: string, instance: string | null): Promise<unknown>;
};

async function openVariables(page: Page) {
  await page.goto("/");
  await expect(
    page.getByTestId("workspace-view").getByText("dev", { exact: true }).first(),
  ).toBeVisible();
  await envHeader(page).getByRole("button", { name: "Variables" }).click();
  await expect(page.getByTestId("variables-dialog")).toContainText("Variables of dev");
}

async function add(page: Page, id: string, key: string, value: string, secret = false) {
  await page.getByTestId(`${id}-key`).fill(key);
  await page.getByTestId(`${id}-value`).fill(value);
  if (secret) await page.getByTestId(`${id}-secret`).click();
  await page.getByTestId(`${id}-set`).click();
}

const row = (page: Page, set: string, key: string) =>
  page.locator(`[data-testid="${set}-row"][data-key="${key}"]`);

test("a secret goes to the keychain, the state holds its reference", async ({ page }) => {
  await openVariables(page);
  await add(page, "vars-environment-add", "REGION", "eu-central-1");
  await expect(row(page, "vars-environment", "REGION")).toContainText("eu-central-1");
  await add(page, "vars-environment-add", "API_TOKEN", "t0k3n-e2e-value", true);
  const token = row(page, "vars-environment", "API_TOKEN");
  await expect(token).toHaveAttribute("data-secret", "true");
  await expect(token).toContainText("keychain:");
  await expect(token).not.toContainText("t0k3n-e2e-value");
  await expect(page.getByText(/API_TOKEN set on the environment/)).toBeVisible();

  await page.getByTestId("vars-tab-local").click();
  await expect(page.getByTestId("variables-dialog")).toContainText("never shared");
  await add(page, "vars-local-add", "MINE", "local-e2e-value", true);
  await expect(row(page, "vars-local", "MINE")).toContainText("@local/MINE");

  // The values are in the keychain, and nowhere in what the state answers.
  const { keychain, stateJson } = await page.evaluate(async () => {
    const w = window as unknown as {
      __LL_VARS__: Vars;
      __LL_FAKE_STATE__: {
        stateWorkspaces(): Promise<{ data: { id: string }[] }>;
        stateWorkspace(id: string): Promise<{ data: { environments: { id: string }[] } }>;
      };
    };
    const [ws] = (await w.__LL_FAKE_STATE__.stateWorkspaces()).data;
    const snapshot = await w.__LL_FAKE_STATE__.stateWorkspace(ws?.id ?? "");
    const env = snapshot.data.environments[0]?.id ?? "";
    return {
      keychain: [...w.__LL_VARS__.keychain.values()],
      stateJson: JSON.stringify([snapshot, await w.__LL_VARS__.get(env, null)]),
    };
  });
  expect(keychain.sort()).toEqual(["local-e2e-value", "t0k3n-e2e-value"]);
  expect(stateJson).not.toContain("t0k3n-e2e-value");
  expect(stateJson).not.toContain("local-e2e-value");
});

test("the head revision's vars are read-only; the draft's go into the next", async ({ page }) => {
  await openVariables(page);
  await page.getByTestId("vars-tab-draft").click();
  await add(page, "vars-draft-add", "FEATURE", "on");
  await expect(row(page, "vars-draft", "FEATURE")).toBeVisible();
  await page.getByTestId("variables-close").click();
  await page.getByRole("tab", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit" }).click();
  await expect(page.getByRole("region", { name: "Revisions of dev" })).toContainText("#1");
  await envHeader(page).getByRole("button", { name: "Variables" }).click();
  await page.getByTestId("vars-tab-draft").click();
  await expect(row(page, "vars-revision", "FEATURE")).toBeVisible();
  await expect(page.getByTestId("vars-revision-unset")).toHaveCount(0);
  await expect(page.getByTestId("variables-dialog")).toContainText("revision 2");
});

test("a failed write says why in a toast", async ({ page }) => {
  await openVariables(page);
  await page.evaluate(() =>
    (window as unknown as { __LL_VARS__: Vars }).__LL_VARS__.failNextWrite(
      "the keychain refused it",
    ),
  );
  await add(page, "vars-environment-add", "API_TOKEN", "nope", true);
  await expect(page.getByText("API_TOKEN: the keychain refused it")).toBeVisible();
  await expect(page.getByTestId("vars-environment-empty")).toBeVisible();
});

test("a provider key set in the Models dialog is a workspace secret", async ({ page }) => {
  await page.goto("/");
  await expect(
    page.getByTestId("workspace-view").getByText("dev", { exact: true }).first(),
  ).toBeVisible();
  // MLflow in dev's draft, as the CLI would add it: the Models selector appears.
  await page.evaluate(async () => {
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
    await state.stateSetSelection(env.id, {
      ...env.selection,
      modules: [...env.selection.modules, "mlflow"],
    });
  });
  await page.getByRole("tab", { name: "History" }).click();
  await page.getByRole("button", { name: "Commit" }).click();
  await envHeader(page).getByRole("button", { name: "Choose models ▾" }).click();
  await expect(page.getByTestId("models-no-keys")).toBeVisible();
  await page.getByTestId("models-key-OPENAI_API_KEY-value").fill("sk-e2e-openai");
  await page.getByTestId("models-key-OPENAI_API_KEY-set").click();
  await expect(page.getByTestId("models-key-OPENAI_API_KEY-where")).toHaveText(
    "Set on the workspace",
  );
  await expect(page.getByTestId("models-no-keys")).toHaveCount(0);
  const keychain = await page.evaluate(() => [
    ...(window as unknown as { __LL_VARS__: Vars }).__LL_VARS__.keychain.entries(),
  ]);
  expect(keychain).toHaveLength(1);
  expect(keychain[0]?.[0]).toMatch(/\/OPENAI_API_KEY$/);
  expect(keychain[0]?.[1]).toBe("sk-e2e-openai");
});
