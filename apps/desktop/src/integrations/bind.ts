// Points the service clients at the host's bindings. main.ts imports this after
// `registerHost`; the bindings resolve on every call, so a later environment
// switch needs nothing here. The paths are the services' own, under the root
// of the binding: the gateway of a local instance (which routes them to the
// modules) or a Neon Function (which serves one service at its root).

import { createConnectTransport } from "@connectrpc/connect-web";
import { registerTransport } from "@headwaters/lineage-client";
import { hostFetch } from "@open-lakehouse/host";
import { setDefaultUnityCatalogFetch } from "@open-lakehouse/unity-catalog-client";
import { HEADWATERS, UNITY_CATALOG } from "./services";

export function bindServiceClients(): void {
  setDefaultUnityCatalogFetch(hostFetch(UNITY_CATALOG));
  registerTransport(
    // The read API is at the binding's root: `/<service>/<method>`.
    createConnectTransport({ baseUrl: "", fetch: hostFetch(HEADWATERS) }),
  );
}
