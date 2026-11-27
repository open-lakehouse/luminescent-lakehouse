import { afterEach, describe, expect, test } from "bun:test";
import { seedQueue } from "@open-lakehouse/factory-client/testing";
import { cleanup, fireEvent, waitFor } from "@testing-library/react";
import { FactoryControlRoom } from "./FactoryControlRoom.js";
import { renderWithFactory } from "./test-utils.js";

afterEach(cleanup);

describe("FactoryControlRoom under a host's factory binding", () => {
  test("lists plans, shows the revision chain, and switches to the diff", async () => {
    const r = renderWithFactory(() => <FactoryControlRoom />);
    await waitFor(() => expect(r.getByRole("list", { name: "Plans" })).toBeDefined());
    expect(r.getAllByText("Typed work graph on olai-store").length).toBeGreaterThan(0);
    await waitFor(() =>
      expect(r.getByTestId("revision-timeline").textContent).toContain("approved"),
    );
    // The service was reached through the host's binding, not a global.
    expect(r.getByTestId("plan-canvas")).toBeDefined();
    fireEvent.mouseDown(r.getByRole("tab", { name: "Diff" }));
    fireEvent.click(r.getByRole("tab", { name: "Diff" }));
    await waitFor(() => expect(r.getAllByRole("treeitem").length).toBeGreaterThan(3));
  });

  test("approving the current revision clears the drift", async () => {
    const r = renderWithFactory(() => <FactoryControlRoom />);
    const approve = await waitFor(() => r.getByRole("button", { name: "Approve current" }));
    fireEvent.click(approve);
    await waitFor(() => expect(r.queryByRole("button", { name: "Approve current" })).toBeNull());
    const root = [...r.fake.graphs.keys()][0];
    expect(r.fake.drifted(r.fake.graphs.get(root) as never)).toEqual([]);
  });

  test("a node with a run shows its stage, PR group and sessions, which open in the host", async () => {
    const opened: string[] = [];
    const r = renderWithFactory(() => (
      <FactoryControlRoom onOpenSession={(s) => opened.push(s.sessionId)} />
    ));
    const card = await waitFor(() => r.getByTestId("run"));
    expect(card.textContent).toContain("in review");
    expect(card.textContent).toContain("#101 1/2");
    expect(card.textContent).toContain("#102 2/2");
    // The demo's gate names its condition through the PrMerged renderer.
    const chips = r.getAllByTestId("payload-chip").map((c) => c.textContent);
    expect(chips).toContain("Merged: Define factory.workgraph.v1");
    const node = card.closest("[data-testid^='work-node-']") as HTMLElement;
    fireEvent.click(node);
    const run = await waitFor(() => r.getByRole("region", { name: "Run" }));
    expect(run.textContent).toContain("task.v1");
    expect(run.textContent).toContain("factory-reviewer");
    expect(run.textContent).toContain("codex");
    expect(run.textContent).toContain("sess-demo-2");
    expect(run.textContent).toContain("in local (registry default)");
    // With the host's hook, a session opens there (the desktop's Agent view).
    const open = run.querySelectorAll("button[aria-label^='Open session']");
    expect(open.length).toBeGreaterThan(0);
    fireEvent.click(open[open.length - 1] as HTMLElement);
    expect(opened).toEqual(["sess-demo-2"]);
  });

  test("a queued run shows its environment and place in line, on its card and in the inspector", async () => {
    let line: string[] = [];
    const r = renderWithFactory(
      () => <FactoryControlRoom />,
      "demo",
      ({ fake }) => {
        line = seedQueue(fake, [...fake.graphs.keys()][0] as string);
      },
    );
    await waitFor(() => expect(r.getAllByTestId("run-queued")).toHaveLength(2));
    // The places load once a run waits, so the line positions follow.
    await waitFor(() =>
      expect(r.getAllByTestId("run-queued").map((c) => c.textContent)).toEqual(
        expect.arrayContaining(["queued · sandbox · 1st of 2", "queued · sandbox · 2nd of 2"]),
      ),
    );
    const first = r.fake.graphs
      .values()
      .next()
      .value?.runs.find((x) => x.ref === line[0]);
    fireEvent.click(r.getByTestId(`work-node-${first?.node}`));
    const waiting = await waitFor(() => r.getByTestId("run-waiting"));
    expect(waiting.textContent).toContain("Waiting for a place in sandbox");
    expect(waiting.textContent).toContain("1st in line, 1 of 1 places taken");
    // The inspector's markdown fields are plain text areas until edited:
    // reading a node doesn't load Monaco.
    expect(r.getAllByRole("textbox").some((t) => t.tagName === "TEXTAREA")).toBe(true);
  });
});
