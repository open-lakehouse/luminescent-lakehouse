// The standalone app's host: one environment, and a `factory` binding that is
// factory-server's Connect API, on this page's origin by default (the Vite
// dev server proxies it) or at `VITE_FACTORY_API`.

import type { LakehouseHost, ServiceBinding } from "@open-lakehouse/host";

export function createWebHost(apiBase = ""): LakehouseHost {
  const factory: ServiceBinding = {
    // Request paths are relative to the service root.
    fetch: ((input: RequestInfo | URL, init?: RequestInit) => {
      const url = input instanceof Request ? input.url : String(input);
      const target = /^https?:/.test(url) ? url : `${apiBase}${url}`;
      return globalThis.fetch(input instanceof Request ? new Request(target, input) : target, init);
    }) as typeof globalThis.fetch,
    placement: "local",
  };
  const env = { id: "local", name: "local" };
  return {
    kind: "web",
    environment: { current: () => env, key: env.id, subscribe: () => () => {} },
    services: {
      get: (key) => (key === "factory" ? factory : undefined),
      list: () => ["factory"],
    },
  };
}
