// The desktop frame on fakes, for the shell's tests: the fake state and host, the
// real editor, and queries on an environment's tab (its editor tab's state, its hover
// card, its sub-tabs, and its actions: the run action at their end and the rest in
// their ⋯ menu).

import { expect } from "bun:test";
import { createFakeHost, type FakeHost, type FakeService } from "@open-lakehouse/host/testing";
import { configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost, type FakeShellHost } from "../host/fake-host";
import { ShellLayout } from "./ShellLayout";

// The frame's DOM (the canvas especially) is large: a failed query says what it
// looked for, without printing the document each time `waitFor` retries.
configure({ getElementError: (message) => new Error(message ?? "not found") });

export type Harness = FakeShellHost & { host: FakeHost; dev: string; workspaceId: string };

/** The frame on `local/dev` (no revision yet), with `dev`'s tab open. `services` are
 *  bound whatever the environment (as the shell's in-process factory is). */
export async function renderFrame(
  options: Parameters<typeof createFakeShellHost>[0] & {
    services?: Record<string, FakeService>;
  } = {},
): Promise<Harness> {
  const host = createFakeHost({ kind: "desktop", environment: null, services: options.services });
  const shell = createFakeShellHost({
    onUse: (instance) => host.setEnvironment({ id: instance, name: "dev" }),
    ...options,
  });
  const ws = await shell.workspaces.create("local", "dev");
  const snapshot = await shell.state.stateWorkspace(ws.id);
  if (snapshot.status !== "ok") throw new Error("snapshot");
  const dev = snapshot.data.environments[0]?.id ?? "";
  render(
    <Providers host={host} shell={shell}>
      <ShellLayout />
    </Providers>,
  );
  await waitFor(() => expect(envState()).not.toBeNull());
  return Object.assign(shell, { host, dev, workspaceId: ws.id });
}

/** The open environment's sub-tabs. */
export const subTabs = () =>
  document.querySelector<HTMLElement>('[role="tablist"][aria-label="Environment views"]');
/** The row the sub-tabs end: the run action and the ⋯ menu. */
const subTabRow = () => subTabs()?.parentElement ?? null;
/** The active editor tab: the open environment's. */
const editorTab = () =>
  document.querySelector<HTMLElement>(
    '[role="tablist"][aria-label="Open editors"] [role="tab"][aria-selected="true"]',
  );
/** The open environment's state, as its editor tab's dot says. */
export const envState = () =>
  editorTab()?.querySelector("[data-run-state]")?.getAttribute("data-run-state") ?? null;

/** The open environment's hover card, if it is open. Asks it to open when it isn't
 *  (it opens after its delay), so `waitFor(() => card())` finds it. */
export function card(): HTMLElement | null {
  const open = document.querySelector<HTMLElement>('[aria-label^="About "]');
  if (open) return open;
  const trigger = editorTab()?.parentElement;
  if (trigger) fireEvent.pointerEnter(trigger, { pointerType: "mouse" });
  return null;
}
/** The open card, once it is there. */
export const openCard = () =>
  waitFor(() => {
    const c = card();
    if (!c) throw new Error("no environment card");
    return c;
  });
/** The card's text (asking it to open). */
export const cardText = () => card()?.textContent ?? "";
/** The card's detail labelled `label` (`Neon`, `Models`, `Gateway`): its row's value. */
function cardDetail(label: string): HTMLElement | undefined {
  for (const dt of card()?.querySelectorAll("dt") ?? []) {
    if (dt.textContent?.trim() === label) return dt.nextElementSibling as HTMLElement;
  }
  return undefined;
}
/** The value of the card's detail `label`, if shown. */
export const cardValue = (label: string) =>
  cardDetail(label)?.querySelector(".font-mono")?.textContent ?? undefined;
/** The card's detail `label` (`Neon`: the branch, `Models`: the models): a button
 *  showing the current choice, which opens Configure on it. */
export function selector(label: "Neon" | "Models"): HTMLButtonElement | undefined {
  return cardDetail(label)?.querySelector("button:not([aria-label^='Copy'])") ?? undefined;
}
/** The choice `selector(label)` shows. */
export const selection = (label: "Neon" | "Models") =>
  selector(label)?.querySelector(".font-mono")?.textContent ?? undefined;
/** Click `selector(label)` once it is there. */
export async function clickSelector(label: "Neon" | "Models") {
  const button = await waitFor(() => {
    const b = selector(label);
    if (!b) throw new Error(`no ${label} detail in the card`);
    return b;
  });
  fireEvent.click(button);
}
/** The Configure tab's section shown (`neon`, `models`, `variables`), if it is open. */
export const configureSection = () =>
  document.querySelector('[data-testid="configure"]')?.getAttribute("data-section") ?? null;
/** Open the Configure tab on `section`. */
export async function openConfigure(section: "neon" | "models" | "variables") {
  fireEvent.click(await waitFor(() => tab("Configure")));
  fireEvent.click(await screen.findByTestId(`configure-nav-${section}`));
  return screen.findByTestId(`configure-${section}`);
}
/** The open environment's run action, at the end of its sub-tabs, if it is `name`
 *  (an open menu hides it from the accessibility tree: this looks past that). */
export function queryAction(name: string | RegExp): HTMLElement | null {
  const row = subTabRow();
  return row ? within(row).queryByRole("button", { name, hidden: true }) : null;
}
export function action(name: string | RegExp): HTMLElement {
  const found = queryAction(name);
  if (!found) throw new Error(`no run action ${name}`);
  return found;
}
/** Whether action `el` can't be run. */
export const isDisabled = (el: HTMLElement) =>
  el.hasAttribute("disabled") || el.getAttribute("aria-disabled") === "true";

/** Open the ⋯ menu ending the open environment's sub-tabs; resolves to the menu. */
export async function openMenu(): Promise<HTMLElement> {
  const open = () => document.querySelector<HTMLElement>('[role="menu"]');
  if (!open()) {
    const row = subTabRow() as HTMLElement;
    const more = within(row).getByRole("button", { name: /^More actions for /, hidden: true });
    fireEvent.pointerDown(more, { button: 0, ctrlKey: false, pointerType: "mouse" });
  }
  return waitFor(() => {
    const menu = open();
    if (!menu) throw new Error("the menu didn't open");
    return menu;
  });
}
/** Close the ⋯ menu, if open. */
export function closeMenu() {
  const menu = document.querySelector<HTMLElement>('[role="menu"]');
  if (menu) fireEvent.keyDown(menu, { key: "Escape" });
}
/** The ⋯ menu's entry `name`, if the menu is open. */
export function queryMenuAction(name: string | RegExp): HTMLElement | null {
  const menu = document.querySelector<HTMLElement>('[role="menu"]');
  return menu ? within(menu).queryByRole("menuitem", { name }) : null;
}
/** Open the ⋯ menu, and its entry `name` once it is there. */
export async function menuAction(name: string | RegExp): Promise<HTMLElement> {
  await openMenu();
  return waitFor(() => {
    const item = queryMenuAction(name);
    if (!item) throw new Error(`no menu action ${name}`);
    return item;
  });
}
/** Run the ⋯ menu's entry `name` once it is enabled. */
export async function clickMenuAction(name: string | RegExp) {
  await openMenu();
  const item = await waitFor(() => {
    const i = queryMenuAction(name);
    if (!i) throw new Error(`no menu action ${name}`);
    expect(isDisabled(i)).toBe(false);
    return i;
  });
  fireEvent.click(item);
}
export const tab = (name: string) => within(subTabs() as HTMLElement).getByRole("tab", { name });
export const queryTab = (name: string) =>
  within(subTabs() as HTMLElement).queryByRole("tab", { name });

export async function commit(shell: Harness, env = shell.dev) {
  const committed = await shell.state.stateCommit(env);
  if (committed.status !== "ok") throw new Error("commit");
}

/** Click the run action once it is `name` and enabled. */
export async function clickAction(name: string | RegExp) {
  const button = await waitFor(() => {
    const b = action(name);
    expect(isDisabled(b)).toBe(false);
    return b;
  });
  fireEvent.click(button);
}

/** The bottom panel's tab `name` (Problems, Run, Logs). */
export const panelTab = (name: string) =>
  within(screen.getByRole("region", { name: "Panel" })).getByRole("tab", { name });

/** Open the bottom panel's Run view. */
export function openRunPanel() {
  fireEvent.click(panelTab("Run"));
}

/** Open the bottom panel's Logs view. */
export function openLogsPanel() {
  fireEvent.click(panelTab("Logs"));
}

/** Open the ⌘K palette and pick the row titled `title`. */
export async function runFromPalette(title: string) {
  fireEvent.keyDown(window, { key: "k", metaKey: true, ctrlKey: true });
  const row = await waitFor(() => screen.getByText(title));
  fireEvent.click(row);
}

/** Lay out the elements matching `selector` at `area` (happy-dom lays out nothing);
 *  returns the undo. */
export function stubRect(
  selector: string,
  area: { left: number; top: number; width: number; height: number },
) {
  const original = HTMLElement.prototype.getBoundingClientRect;
  HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
    if (!this.matches(selector)) return original.call(this);
    return {
      ...area,
      x: area.left,
      y: area.top,
      right: area.left + area.width,
      bottom: area.top + area.height,
      toJSON: () => area,
    } as DOMRect;
  };
  return () => {
    HTMLElement.prototype.getBoundingClientRect = original;
  };
}

/** The Apps view's sidebar, while the view is open. */
export const appsSidebar = () => screen.getByTestId("apps-sidebar");
/** The Apps view's item in the activity bar: opens it, or goes back to the editor. */
export const appsActivity = () =>
  within(screen.getByLabelText("Views")).getByRole("button", { name: "Apps" });
/** App `name` of the instance selected in the Apps view. */
export const appButton = (name: string) => within(appsSidebar()).getByRole("button", { name });
/** Whether the editor tabs are hidden behind a workbench view. */
export const editorHidden = () =>
  document.querySelector<HTMLElement>("[data-workbench-editor]")?.hidden ?? false;
