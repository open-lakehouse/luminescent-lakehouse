// The Configure tab's work-graph agents, on fakes: roles from the branch's catalog,
// a route per role, keys that go in once and never come back out, and no way to put the
// apps token on a route.

import { afterEach, expect, test } from "bun:test";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost } from "../host/fake-host";
import type { FakeWorkgraphAgentsOptions } from "../host/fake-workgraph-agents";
import { WorkgraphAgentsForm } from "./WorkgraphAgentsForm";

afterEach(cleanup);

const KEY = `olk_0123456789abcdef_${"ab".repeat(32)}`;
const APPS_TOKEN = "wg_desk_0123456789abcdef0123456789abcdef";

/** Let the mutations and the dialog's close run. */
const settle = (ms = 300) => new Promise((resolve) => setTimeout(resolve, ms));

function renderForm(options: FakeWorkgraphAgentsOptions = {}, locked?: string) {
  const shell = createFakeShellHost({ workgraphAgents: options });
  render(
    <Providers host={createFakeHost({ kind: "desktop", environment: null })} shell={shell}>
      <WorkgraphAgentsForm environmentId="env-1" locked={locked} />
    </Providers>,
  );
  return shell;
}

test("the roles come from the branch's function, and each one on gets a route with its key", async () => {
  const shell = renderForm();
  const planner = await screen.findByTestId("agent-role-planner");
  expect(screen.getByTestId("agent-role-reviewer")).toBeDefined();
  expect(screen.getByTestId("egress-routes-none")).toBeDefined();
  fireEvent.click(planner);
  fireEvent.click(screen.getByTestId("workgraph-agents-save"));
  const route = await screen.findByTestId("egress-route-workgraph-planner");
  expect(route.textContent).toContain("workgraph function");
  expect(route.textContent).toContain("/mcp");
  expect(route.textContent).toContain("omnigent");
  expect(within(route).getByTestId("egress-route-workgraph-planner-key").textContent).toBe("set");
  expect(shell.workgraphAgents.calls).toEqual(["enable env-1 planner"]);

  fireEvent.click(screen.getByTestId("egress-route-workgraph-planner-rotate"));
  await waitFor(() => expect(shell.workgraphAgents.calls).toContain("rotate env-1 planner"));
});

test("a pasted key is taken once and never shown; the apps token is refused", async () => {
  const shell = renderForm();
  fireEvent.click(await screen.findByTestId("agent-role-reviewer"));
  fireEvent.click(screen.getByTestId("workgraph-agents-save"));
  fireEvent.click(await screen.findByTestId("egress-route-workgraph-reviewer-paste"));

  const input = (await screen.findByTestId("paste-key-value")) as HTMLInputElement;
  expect(input.type).toBe("password");
  fireEvent.change(input, { target: { value: APPS_TOKEN } });
  fireEvent.click(screen.getByTestId("paste-key-save"));
  const error = await screen.findByTestId("paste-key-error");
  expect(error.textContent).toContain("never the apps token");
  // The refused value is gone from the field too.
  expect((screen.getByTestId("paste-key-value") as HTMLInputElement).value).toBe("");

  fireEvent.change(screen.getByTestId("paste-key-value"), { target: { value: KEY } });
  fireEvent.click(screen.getByTestId("paste-key-save"));
  // Not `waitFor`: polling while Radix closes the dialog crashes bun's test DOM.
  await settle();
  expect(screen.queryByTestId("paste-key")).toBeNull();
  expect(shell.workgraphAgents.calls.filter((c) => c.startsWith("setKey"))).toEqual([
    "setKey env-1 reviewer",
    "setKey env-1 reviewer",
  ]);
  expect(document.body.innerHTML).not.toContain(KEY);
  expect(document.body.innerHTML).not.toContain(APPS_TOKEN);
});

test("every egress route shows, and nothing is offered without Omnigent", async () => {
  renderForm({
    omnigent: () => false,
    otherRoutes: [
      {
        name: "tickets",
        function: null,
        upstream: "https://tickets.example",
        path: "/",
        consumers: ["unity-catalog-oss"],
        secret: "TICKETS_API_KEY",
        keySet: false,
        tools: [],
        role: null,
      },
    ],
  });
  const tickets = await screen.findByTestId("egress-route-tickets");
  expect(tickets.textContent).toContain("https://tickets.example");
  expect(within(tickets).getByTestId("egress-route-tickets-key").textContent).toBe("missing");
  // A route this section didn't make has no key actions here.
  expect(screen.queryByTestId("egress-route-tickets-paste")).toBeNull();
  expect(screen.getByTestId("workgraph-agents-blocked").textContent).toContain("Omnigent");
  expect((screen.getByTestId("workgraph-agents-save") as HTMLButtonElement).disabled).toBe(true);
});

test("while the environment runs, the roles are locked but keys can still change", async () => {
  renderForm({}, "Stop dev to change its agents' roles: Start binds it");
  const planner = await screen.findByTestId("agent-role-planner");
  expect(planner.closest("fieldset")?.disabled).toBe(true);
});
