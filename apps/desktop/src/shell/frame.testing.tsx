// The desktop frame on fakes, for the shell's tests: the fake state and host, the
// real editor, and queries on an environment's tab (its header, sub-tabs and
// actions).

import { expect } from "bun:test";
import { createFakeHost, type FakeHost } from "@open-lakehouse/host/testing";
import { configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost, type FakeShellHost } from "../host/fake-host";
import { ShellLayout } from "./ShellLayout";

// The frame's DOM (the canvas especially) is large: a failed query says what it
// looked for, without printing the document each time `waitFor` retries.
configure({ getElementError: (message) => new Error(message ?? "not found") });

export type Harness = FakeShellHost & { host: FakeHost; dev: string; workspaceId: string };

/** The frame on `local/dev` (no revision yet), with `dev`'s tab open. */
export async function renderFrame(
  options: Parameters<typeof createFakeShellHost>[0] = {},
): Promise<Harness> {
  const host = createFakeHost({ kind: "desktop", environment: null });
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
/** The open environment's header: state, revision, placement, actions. */
export const header = () => subTabs()?.previousElementSibling as HTMLElement | null | undefined;
/** The open environment's state, as its header's dot says. */
export const envState = () =>
  header()?.querySelector("[data-run-state]")?.getAttribute("data-run-state") ?? null;
/** The header detail labelled `label` (e.g. "Neon", "Gateway"): its value, if shown. */
export function headerDetail(label: string): string | undefined {
  for (const fact of header()?.querySelectorAll("span[title]") ?? []) {
    const [name, value] = fact.children;
    if (name?.textContent === label) return value?.textContent ?? undefined;
  }
  return undefined;
}
export const action = (name: string | RegExp) =>
  within(header() as HTMLElement).getByRole("button", { name });
export const queryAction = (name: string | RegExp) =>
  within(header() as HTMLElement).queryByRole("button", { name });
export const tab = (name: string) => within(subTabs() as HTMLElement).getByRole("tab", { name });
export const queryTab = (name: string) =>
  within(subTabs() as HTMLElement).queryByRole("tab", { name });

export async function commit(shell: Harness, env = shell.dev) {
  const committed = await shell.state.stateCommit(env);
  if (committed.status !== "ok") throw new Error("commit");
}

/** Click `name` in the header once it is enabled. */
export async function clickAction(name: string | RegExp) {
  const button = await waitFor(() => {
    const b = action(name) as HTMLButtonElement;
    expect(b.disabled).toBe(false);
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
