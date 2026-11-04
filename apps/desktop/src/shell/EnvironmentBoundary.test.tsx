// Switching between two running instances never leaks data between them: what a
// view fetched from one environment (cached, or still in flight) never renders
// under the next. The Catalog and Lineage views sit under this boundary and fetch
// through their binding the same way; the Docker smoke checks them end to end.

import { afterEach, expect, test } from "bun:test";
import { useEnvironmentKey, useService } from "@open-lakehouse/host/react";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { QueryClient, useQuery } from "@tanstack/react-query";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { Providers } from "../app";
import { createFakeShellHost } from "../host/fake-host";
import { EnvironmentBoundary } from "./ShellLayout";

afterEach(cleanup);

const CATALOGS = "/api/2.1/unity-catalog/catalogs";

/** What each render showed, under which environment key. */
const renders: { key: string; names: string[] }[] = [];

function CatalogProbe() {
  const key = useEnvironmentKey();
  const binding = useService("unity-catalog");
  const catalogs = useQuery({
    queryKey: ["catalogs"],
    queryFn: async () => {
      const response = await (binding as NonNullable<typeof binding>).fetch(CATALOGS);
      return ((await response.json()) as { catalogs: { name: string }[] }).catalogs;
    },
  });
  const names = catalogs.data?.map((c) => c.name) ?? [];
  renders.push({ key, names });
  return <p data-testid="catalogs">{names.join(",") || "loading"}</p>;
}

/** A Unity Catalog that answers `name` once `release` is called (at once without). */
function catalog(name: string) {
  let release!: () => void;
  const gate = new Promise<void>((r) => {
    release = r;
  });
  return {
    release,
    open: () => release(),
    handler: async () => {
      await gate;
      return Response.json({ catalogs: [{ name }] });
    },
  };
}

function renderProbe() {
  const a = catalog("alpha");
  const host = createFakeHost({
    kind: "desktop",
    environment: { id: "instance-a", name: "dev" },
    services: { "unity-catalog": a.handler },
  });
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <Providers host={host} shell={createFakeShellHost()} queryClient={queryClient}>
      <EnvironmentBoundary>
        <CatalogProbe />
      </EnvironmentBoundary>
    </Providers>,
  );
  return { host, a, queryClient };
}

/** As the desktop switches: rebind first, then announce the new environment. */
function switchTo(
  host: ReturnType<typeof createFakeHost>,
  id: string,
  handler: () => Promise<Response>,
) {
  act(() => {
    host.setService("unity-catalog", handler);
    host.setEnvironment({ id, name: "dev" });
  });
}

test("a switch shows the next instance's data, never the previous one's", async () => {
  renders.length = 0;
  const { host, a } = renderProbe();
  a.open();
  await waitFor(() => expect(screen.getByTestId("catalogs").textContent).toBe("alpha"));
  const keyA = host.environment.key;

  const b = catalog("beta");
  switchTo(host, "instance-b", b.handler);
  const keyB = host.environment.key;
  expect(keyB).not.toBe(keyA);
  b.open();
  await waitFor(() => expect(screen.getByTestId("catalogs").textContent).toBe("beta"));

  const underB = renders.filter((r) => r.key === keyB);
  expect(underB.length).toBeGreaterThan(0);
  expect(underB.some((r) => r.names.includes("alpha"))).toBe(false);
  // B's view asked B's binding.
  expect(host.requests.at(-1)?.request.url).toContain(CATALOGS);
});

test("a response still in flight from the previous instance is dropped", async () => {
  renders.length = 0;
  const { host, a, queryClient } = renderProbe();
  // A's request is out, unanswered, when the switch comes.
  await waitFor(() => expect(host.requests.length).toBe(1));
  const b = catalog("beta");
  switchTo(host, "instance-b", b.handler);
  const keyB = host.environment.key;

  a.open(); // A answers late.
  await new Promise((r) => setTimeout(r, 20));
  expect(screen.getByTestId("catalogs").textContent).toBe("loading");
  b.open();
  await waitFor(() => expect(screen.getByTestId("catalogs").textContent).toBe("beta"));

  expect(renders.filter((r) => r.key === keyB).some((r) => r.names.includes("alpha"))).toBe(false);
  expect(queryClient.getQueryData(["catalogs"])).toEqual([{ name: "beta" }]);
});
