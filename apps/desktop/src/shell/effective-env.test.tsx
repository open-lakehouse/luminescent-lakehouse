// The bottom panel's Environment view, in the frame on the fake state and fake vars:
// each variable with its layer and what it shadows, a Neon branch's keys labelled with
// the branch, secrets masked with their reference, and the masked dotenv on the
// clipboard.

import { afterEach, beforeEach, expect, test } from "bun:test";
import { cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import {
  clickAction,
  commit,
  envState,
  panelTab,
  renderFrame,
  runFromPalette,
} from "./frame.testing";

let copied: string[] = [];
beforeEach(() => {
  localStorage.clear();
  sessionStorage.clear();
  copied = [];
  Object.defineProperty(navigator, "clipboard", {
    configurable: true,
    value: { writeText: async (text: string) => void copied.push(text) },
  });
});
afterEach(cleanup);

const row = (key: string) =>
  screen.getAllByTestId("effective-row").find((r) => r.getAttribute("data-key") === key);

test("before a Start there is nothing to show", async () => {
  await renderFrame();
  fireEvent.click(panelTab("Environment"));
  expect((await screen.findByTestId("effective-none")).textContent).toContain("start it");
});

test("each variable shows its layer, what it shadows, and a secret only by reference", async () => {
  const shell = await renderFrame();
  const local = { scope: "local", environment: shell.dev } as const;
  await shell.vars.set({ scope: "workspace", workspace: shell.workspaceId }, "LEVEL", {
    kind: "literal",
    value: "info",
  });
  await shell.vars.set(local, "LEVEL", { kind: "literal", value: "debug" });
  await shell.vars.set(local, "TOKEN", { kind: "secret", value: "s3cret-token" });
  await commit(shell);
  await clickAction("Start");
  await waitFor(() => expect(envState()).toBe("running"));
  fireEvent.click(panelTab("Environment"));
  await screen.findByTestId("effective-table");

  const level = row("LEVEL") as HTMLElement;
  expect(level.getAttribute("data-layer")).toBe("local vars");
  expect(level.textContent).toContain("debug");
  expect(within(level).getByTestId("effective-shadowed").textContent).toBe("over workspace vars");
  const token = row("TOKEN") as HTMLElement;
  expect(token.getAttribute("data-secret")).toBe("true");
  expect(token.textContent).toContain("••••••");
  expect(token.textContent).toContain(`keychain:${shell.dev}@local/TOKEN`);
  expect(screen.getByTestId("effective-panel").textContent).not.toContain("s3cret-token");

  // The filter narrows it, by name or layer.
  fireEvent.change(screen.getByTestId("effective-filter"), { target: { value: "local vars" } });
  await waitFor(() =>
    expect(screen.getAllByTestId("effective-row").map((r) => r.getAttribute("data-key"))).toEqual([
      "LEVEL",
      "TOKEN",
    ]),
  );

  // An edit shows at once, and the copy is masked.
  await shell.vars.set(local, "LEVEL", { kind: "literal", value: "trace" });
  fireEvent.click(screen.getByTestId("effective-refresh"));
  await waitFor(() => expect(row("LEVEL")?.textContent).toContain("trace"));
  fireEvent.click(screen.getByTestId("effective-copy"));
  await waitFor(() => expect(copied).toHaveLength(1));
  expect(copied[0]).toContain("# local vars\nLEVEL=trace\n");
  expect(copied[0]).toContain("TOKEN=••••••");
  expect(copied[0]).not.toContain("s3cret-token");
});

test("a Neon branch's keys show the branch as their layer", async () => {
  const shell = await renderFrame();
  await commit(shell);
  const started = await shell.state.runStart(shell.dev, false, {
    kind: "existing",
    branch: "staging",
  });
  if (started.status === "error") throw new Error("no start");
  await waitFor(() => expect(envState()).toBe("running"));
  await runFromPalette("Show dev's effective environment");
  await screen.findByTestId("effective-table");
  const pg = row("PGHOST") as HTMLElement;
  expect(pg.getAttribute("data-layer")).toBe("source:neon");
  expect(pg.textContent).toContain("Neon branch staging");
  expect(row("DATABASE_URL")?.textContent).toContain("••••••");
});

test("a long value is truncated in its fixed-width column, and a click wraps it", async () => {
  const shell = await renderFrame();
  const long = `https://${"a".repeat(400)}.example.com`;
  await shell.vars.set({ scope: "local", environment: shell.dev }, "LONG_URL", {
    kind: "literal",
    value: long,
  });
  await commit(shell);
  await clickAction("Start");
  await waitFor(() => expect(envState()).toBe("running"));
  fireEvent.click(panelTab("Environment"));
  expect((await screen.findByTestId("effective-table")).className).toContain("table-fixed");
  const value = within(row("LONG_URL") as HTMLElement).getByTestId("effective-value");
  expect(value.className).toContain("truncate");
  fireEvent.click(value);
  expect(value.getAttribute("data-wrapped")).toBe("true");
  fireEvent.click(within(row("LONG_URL") as HTMLElement).getByTestId("effective-value-copy"));
  await waitFor(() => expect(copied).toEqual([long]));
});
