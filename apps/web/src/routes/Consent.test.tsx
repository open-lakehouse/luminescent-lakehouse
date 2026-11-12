import { afterEach, beforeEach, describe, expect, mock, test } from "bun:test";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";

// The branch's Auth server, faked: a signed-in user, a named client, and a
// record of the consent answers.
const answers: boolean[] = [];
mock.module("@/lib/auth", () => ({
  useSession: () => ({
    data: { user: { email: "ada@example.com" } },
    isPending: false,
    error: null,
  }),
  publicClient: async (_endpoint: string, client_id: string) => ({
    client_id,
    client_name: "luminescent CLI",
  }),
  consent: async (_endpoint: string, accept: boolean) => {
    answers.push(accept);
    return undefined;
  },
  continueAuthorization: async () => undefined,
  signIn: async () => {},
  signOut: async () => {},
}));

const { default: App } = await import("../App");

const EP = "ep-cool-darkness-a1b2c3d4";
const SUFFIX = "neonauth.c-7.eu-central-1.aws.neon.tech";
const BASE = `/a/${EP}.${SUFFIX}/neondb`;
const signed = (redirect: string) =>
  `?client_id=cli&redirect_uri=${encodeURIComponent(redirect)}&scope=openid+offline_access` +
  `&resource=${encodeURIComponent("https://headwaters.luminescent-lakehouse.dev")}&sig=s`;

function at(path: string) {
  render(
    <MemoryRouter initialEntries={[path]}>
      <App />
    </MemoryRouter>,
  );
}

beforeEach(() => {
  answers.length = 0;
});
afterEach(cleanup);

describe("consent page", () => {
  test("shows the client, where the code goes, and what it asks for", async () => {
    at(`${BASE}/auth/consent${signed("https://app.example.com/cb")}`);
    expect(await screen.findByText("Allow luminescent CLI?")).toBeTruthy();
    expect(screen.getByText("app.example.com")).toBeTruthy();
    expect(screen.getByText("openid offline_access")).toBeTruthy();
    expect(screen.getByText("https://headwaters.luminescent-lakehouse.dev")).toBeTruthy();
  });

  test("warns when only a loopback address receives the code", async () => {
    at(`${BASE}/auth/consent${signed("http://127.0.0.1:8765/callback")}`);
    expect((await screen.findByRole("note")).textContent).toContain("on this computer");
  });

  test("doesn't warn for a remote redirect", async () => {
    at(`${BASE}/auth/consent${signed("https://app.example.com/cb")}`);
    await screen.findByText("Allow luminescent CLI?");
    expect(screen.queryByRole("note")).toBeNull();
  });

  test("Deny answers the consent with accept: false", async () => {
    at(`${BASE}/auth/consent${signed("http://127.0.0.1:8765/callback")}`);
    const button = await screen.findByRole("button", { name: /deny/i });
    await act(async () => fireEvent.click(button));
    expect(answers).toEqual([false]);
  });

  test("Allow answers the consent with accept: true", async () => {
    at(`${BASE}/auth/consent${signed("https://app.example.com/cb")}`);
    const button = await screen.findByRole("button", { name: /allow/i });
    await act(async () => fireEvent.click(button));
    expect(answers).toEqual([true]);
  });

  test("without a signed request there is nothing to approve", async () => {
    at(`${BASE}/auth/consent?client_id=cli&redirect_uri=https%3A%2F%2Fa.dev%2F`);
    expect(await screen.findByText("Nothing to approve")).toBeTruthy();
  });
});

describe("the branch in the path", () => {
  const notFound = (path: string) => {
    at(path);
    expect(screen.getByText("Not found")).toBeTruthy();
    cleanup();
  };

  test("a host that isn't a Neon Auth server is a 404 on the auth pages", () => {
    for (const host of [
      "ep-a.evil.com",
      "evil.com",
      `${EP}.${SUFFIX}.evil.com`,
      `${EP}.neonauth.example.com`,
    ]) {
      notFound(`/a/${host}/neondb/auth/consent${signed("https://app.example.com/cb")}`);
      notFound(`/a/${host}/neondb/auth/sign-in?sig=s`);
    }
  });

  test("so is a database that isn't one path segment", () => {
    notFound(`/a/${EP}.${SUFFIX}/neon%2Fdb/auth/sign-in?sig=s`);
    notFound(`/a/${EP}.${SUFFIX}/auth/sign-in?sig=s`);
  });

  test("the retired /b/<endpoint-id>/ pages are a 404", () => {
    notFound(`/b/${EP}/auth/consent${signed("https://app.example.com/cb")}`);
    notFound(`/b/${EP}/auth/sign-in?sig=s`);
  });
});
