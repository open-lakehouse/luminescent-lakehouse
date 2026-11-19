import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, fireEvent, waitFor } from "@testing-library/react";
import { renderWithFactory } from "../test-utils.js";
import { PlanDiffView } from "./PlanDiffView.js";

afterEach(cleanup);

describe("PlanDiffView", () => {
  test("renders the server's classification, with ghosts for moved and removed nodes", async () => {
    const r = renderWithFactory((h) => {
      const root = [...h.fake.graphs.keys()][0];
      const g = h.fake.graphs.get(root);
      if (!g?.approved) throw new Error("demo is approved");
      return <PlanDiffView root={root} base={g.approved} target={h.fake.tree(g).subtreeHash} />;
    });
    await waitFor(() => expect(r.getAllByRole("treeitem").length).toBeGreaterThan(3));
    const changes = r.getAllByRole("treeitem").map((el) => el.getAttribute("data-change"));
    expect(changes).toContain("added");
    expect(changes).toContain("modified");
    expect(changes).toContain("changed below");
    // Counts per change kind, as filter chips.
    expect(r.getByRole("group", { name: "Filter changes" }).textContent).toContain("added");
  });

  test("filters, steps through changes with the keyboard, and selects", async () => {
    let selected: string | undefined;
    const s = renderWithFactory((h) => {
      const root = [...h.fake.graphs.keys()][0];
      const g = h.fake.graphs.get(root);
      return (
        <PlanDiffView
          root={root}
          base={g?.approved ?? ""}
          target={h.fake.tree(g as never).subtreeHash}
          onSelect={(k) => {
            selected = k;
          }}
        />
      );
    });
    await waitFor(() => expect(s.getAllByRole("treeitem").length).toBeGreaterThan(3));
    fireEvent.keyDown(s.getByRole("tree"), { key: "j" });
    expect(selected).toBeDefined();
    const chip = s.getAllByRole("button", { pressed: true })[0];
    fireEvent.click(chip);
    expect(chip.getAttribute("aria-pressed")).toBe("false");
  });

  test("an unchanged subtree collapses to one row", async () => {
    const r = renderWithFactory((h) => {
      const root = [...h.fake.graphs.keys()][0];
      const g = h.fake.graphs.get(root);
      const target = h.fake.tree(g as never).subtreeHash;
      return <PlanDiffView root={root} base={target} target={target} />;
    });
    await waitFor(() => expect(r.getByText("No changes")).toBeDefined());
    const rows = r.getAllByRole("treeitem");
    expect(rows.some((el) => el.textContent?.includes("unchanged"))).toBe(true);
  });

  test("one mode offers no switch", async () => {
    const r = renderWithFactory((h) => {
      const root = [...h.fake.graphs.keys()][0];
      const g = h.fake.graphs.get(root);
      return (
        <PlanDiffView
          root={root}
          base={g?.approved ?? ""}
          target={h.fake.tree(g as never).subtreeHash}
          mode="text"
          modes={["tree"]}
        />
      );
    });
    await waitFor(() => expect(r.getAllByRole("treeitem").length).toBeGreaterThan(3));
    expect(r.queryByRole("tab", { name: "Text" })).toBeNull();
  });
});
