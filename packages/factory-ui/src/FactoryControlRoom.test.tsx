import { afterEach, describe, expect, test } from "bun:test";
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

  test("a node with a run shows its stage, PR group and sessions", async () => {
    const r = renderWithFactory(() => <FactoryControlRoom />);
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
  });
});
