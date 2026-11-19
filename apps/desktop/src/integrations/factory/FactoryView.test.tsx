// The Factory app on fakes: the control room over factory-client's in-memory factory
// (the demo plan) behind the host's `factory` binding, and the scripted Omnigent behind
// the session commands for "Open session".

import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { createFakeFactory } from "@open-lakehouse/factory-client/testing";
import { createFakeHost, type FakeHost } from "@open-lakehouse/host/testing";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { Providers } from "../../app";
import { createFakeAgent } from "../../host/fake-agent";
import { createFakeShellHost } from "../../host/fake-host";
import { FactoryView, type SessionTarget } from "./FactoryView";

// Monaco (the inspector's body editor) loads workers bun can't: render no editor.
mock.module("../../../../../packages/factory-ui/src/monaco/index.ts", () => ({
  MarkdownEditor: () => null,
  TextDiff: () => null,
  NodeDiff: () => null,
}));

beforeEach(() => sessionStorage.clear());
afterEach(cleanup);

const TARGET: SessionTarget = { instance: "i1", label: "dev #1", servers: [] };

function renderFactory(options: { sessions?: SessionTarget; bound?: boolean } = {}) {
  const factory = createFakeFactory({ seed: "demo" });
  const host: FakeHost = createFakeHost({
    kind: "desktop",
    environment: null,
    services:
      options.bound === false
        ? {}
        : { factory: { transport: factory.transport }, omnigent: () => new Response() },
  });
  const agent = createFakeAgent({
    sessions: {
      "sess-demo-2": [
        {
          id: "a0",
          role: "assistant",
          parts: [{ type: "text", text: "Reviewed: one nit.", state: "done" }],
        },
      ],
    },
  });
  const shell = createFakeShellHost({
    agent,
    onUse: (instance) => host.setEnvironment({ id: instance, name: "dev" }),
  });
  render(
    <Providers host={host} shell={shell}>
      <FactoryView sessions={"sessions" in options ? options.sessions : TARGET} />
    </Providers>,
  );
  return { host, agent, factory };
}

/** Select the demo's node that has a run, and open its last session. */
async function openSession() {
  const card = await screen.findByTestId("run");
  fireEvent.click(card.closest("[data-testid^='work-node-']") as HTMLElement);
  const run = await screen.findByRole("region", { name: "Run" });
  const open = within(run).getAllByRole("button", { name: /^Open session/ });
  fireEvent.click(open[open.length - 1] as HTMLElement);
}

describe("the Factory app", () => {
  test("shows the demo plan from the factory binding", async () => {
    renderFactory();
    await screen.findByTestId("factory-control-room");
    await waitFor(() =>
      expect(screen.getAllByText("Typed work graph on olai-store").length).toBeGreaterThan(0),
    );
  });

  test("says so when nothing binds the factory", async () => {
    renderFactory({ bound: false });
    expect(await screen.findByTestId("unbound-factory")).toBeDefined();
  });

  test("opens a run's session in the Agent view, and goes back", async () => {
    const { host, agent } = renderFactory();
    await openSession();
    expect(await screen.findByText("Reviewed: one nit.")).toBeDefined();
    // On the target instance, which it made current, as the session's agent.
    expect(host.environment.current()?.id).toBe("i1");
    expect(agent.agents).toEqual(["factory-reviewer"]);
    expect(screen.queryByRole("button", { name: /New chat/ })).toBeNull();
    expect(screen.getByTestId("factory-control-room").closest(".hidden")).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /Control room/ }));
    await waitFor(() => expect(screen.queryByTestId("factory-session")).toBeNull());
    // The control room kept its selection over the switch.
    expect(screen.getByRole("region", { name: "Run" })).toBeDefined();
  });

  test("without an instance with Omnigent, Open session says to start one", async () => {
    renderFactory({ sessions: undefined });
    await openSession();
    expect(await screen.findByTestId("factory-no-omnigent")).toBeDefined();
  });
});
