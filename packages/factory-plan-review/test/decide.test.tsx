import { afterEach, describe, expect, test } from "bun:test";
import { cleanup, fireEvent, render, waitFor } from "@testing-library/react";
import fixture from "../../../apps/desktop/e2e/fixtures/open_plan_review.json";
import { approve, approvedText, type ReviewHost, returnedText, returnPlan } from "../src/decide.js";
import { PlanReviewView } from "../src/PlanReviewView.js";
import { parseReview } from "../src/snapshot.js";

afterEach(cleanup);

const review = (() => {
  const r = parseReview(fixture.structuredContent);
  if (!r) throw new Error("the fixture is a plan review");
  return r;
})();

/** A host that records the calls, and fails the ones named in `fail`. */
function fakeHost(fail: Record<string, string> = {}) {
  const calls: { name: string; args: Record<string, unknown> }[] = [];
  const context: string[] = [];
  const host: ReviewHost = {
    async callTool(name, args) {
      calls.push({ name, args });
      const error = fail[name];
      return error
        ? { isError: true, content: [{ type: "text", text: error }] }
        : { content: [{ type: "text", text: "ok" }] };
    },
    async updateModelContext(text) {
      context.push(text);
    },
  };
  return { host, calls, context };
}

describe("the decision", () => {
  test("approving calls approve_revision at the reviewed revision and tells the model", async () => {
    const h = fakeHost();
    const outcome = await approve(h.host, review);
    expect(outcome.ok).toBe(true);
    expect(h.calls).toEqual([
      { name: "approve_revision", args: { root: review.root, revision: review.revision } },
    ]);
    expect(h.context).toEqual([approvedText(review)]);
    expect(h.context[0]).toStartWith("Approved revision ");
  });

  test("returning needs notes, and carries them", async () => {
    const h = fakeHost();
    expect((await returnPlan(h.host, review, "  ")).ok).toBe(false);
    expect(h.calls).toEqual([]);
    const outcome = await returnPlan(h.host, review, " Split the server task. ");
    expect(outcome.ok).toBe(true);
    expect(h.calls[0]).toEqual({
      name: "return_plan",
      args: { root: review.root, revision: review.revision, notes: "Split the server task." },
    });
    expect(h.context).toEqual([returnedText(review, "Split the server task.")]);
  });

  test("a refused decision tells the person, not the model", async () => {
    const h = fakeHost({ approve_revision: "abc isn't the current revision (def)" });
    const outcome = await approve(h.host, review);
    expect(outcome).toEqual({ ok: false, error: "abc isn't the current revision (def)" });
    expect(h.context).toEqual([]);
  });
});

describe("PlanReviewView", () => {
  test("shows the changes, and approves from the button", async () => {
    const h = fakeHost();
    const r = render(<PlanReviewView review={review} host={h.host} />);
    expect(r.getByRole("heading").textContent).toBe(review.title);
    // The tree diff, from the snapshot's DiffRevisions.
    await waitFor(() => expect(r.getAllByRole("treeitem").length).toBeGreaterThan(3));
    expect(r.queryByRole("tab", { name: "Text" })).toBeNull();
    fireEvent.click(r.getByRole("button", { name: /^Approve revision/ }));
    await waitFor(() => expect(r.getByRole("status").textContent).toContain("Approved"));
    expect(h.calls.map((c) => c.name)).toEqual(["approve_revision"]);
  });

  test("returns with notes, and shows a refusal", async () => {
    const h = fakeHost({ return_plan: "the plan is gone" });
    const r = render(<PlanReviewView review={review} host={h.host} />);
    const ret = r.getByRole("button", { name: "Return with notes" });
    expect(ret.hasAttribute("disabled")).toBe(true);
    fireEvent.change(r.getByRole("textbox", { name: "Notes for the planner" }), {
      target: { value: "Split it." },
    });
    fireEvent.click(ret);
    await waitFor(() => expect(r.getByRole("alert").textContent).toBe("the plan is gone"));
    expect(h.context).toEqual([]);
  });
});
