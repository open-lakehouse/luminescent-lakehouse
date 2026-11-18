// How factory.v1 RPCs reach the control plane. The UI never builds a
// transport; it calls a client over `clientTransport` (or `transportFor(host)`),
// which picks the transport again on every call:
//
//   1. a transport passed explicitly to `createFactoryClient(transport)`
//      (that one never comes through here);
//   2. the one installed with `registerTransport`, an explicit override;
//   3. the `factory` binding of the `@open-lakehouse/host` host: its ConnectRPC
//      `transport`, or else Connect over its `fetch`. The desktop binds the
//      latter through its Rust `service_fetch` proxy; a web host binds
//      factory-server's origin; Storybook binds an in-memory fake;
//   4. with no host at all, Connect over the platform fetch against the page's
//      own origin.
//
// A host that is there but doesn't bind the factory fails the call with
// `ServiceUnavailableError` rather than guessing an origin. Since every call
// resolves again, registering a host before or after a client is built, or
// switching environments, needs no new client.
//
// Messages travel in the binary format: `google.protobuf.Any` payloads of
// kinds the client doesn't know then pass through intact, which the JSON
// format can't do without a registry entry for each.

import type { Transport } from "@connectrpc/connect";
import { createConnectTransport } from "@connectrpc/connect-web";
import {
  getHost,
  type LakehouseHost,
  type ServiceBinding,
  ServiceUnavailableError,
} from "@open-lakehouse/host";

/** The service key the factory is bound under in a host's directory. */
export const FACTORY_SERVICE_KEY = "factory";

/** Connect over fetch, rooted at `baseUrl` (default: the page's origin). */
export function createDefaultTransport(
  baseUrl = "/",
  fetch: typeof globalThis.fetch = globalThis.fetch,
): Transport {
  return createConnectTransport({
    baseUrl,
    useBinaryFormat: true,
    // Bound, so a host's fetch keeps its `this` (the platform one needs it).
    fetch: ((input: RequestInfo | URL, init?: RequestInit) =>
      fetch(input, init)) as typeof globalThis.fetch,
  });
}

let defaultTransport: Transport | null = null;
let registered: Transport | null = null;

/** Install a transport that wins over the host's binding, or clear it. */
export function registerTransport(transport: Transport | null): void {
  registered = transport;
}

const overFetch = new WeakMap<ServiceBinding, Transport>();

function bindingTransport(binding: ServiceBinding): Transport {
  // The host types `transport` structurally; any connect v2 Transport fits.
  if (binding.transport) return binding.transport as Transport;
  let transport = overFetch.get(binding);
  if (!transport) {
    // Paths sit at the binding's root: `/factory.v1.FactoryService/<Method>`.
    transport = createDefaultTransport("", binding.fetch);
    overFetch.set(binding, transport);
  }
  return transport;
}

function resolve(host: LakehouseHost | null): Transport {
  if (registered) return registered;
  if (!host) {
    defaultTransport ??= createDefaultTransport();
    return defaultTransport;
  }
  const binding = host.services.get(FACTORY_SERVICE_KEY);
  if (!binding) {
    throw new ServiceUnavailableError(
      FACTORY_SERVICE_KEY,
      "the current environment doesn't bind it",
    );
  }
  return bindingTransport(binding);
}

/**
 * A transport that resolves on every call, against `host` when one is given
 * (a `<HostProvider>`'s) and against the registered host otherwise.
 */
export function transportFor(host?: LakehouseHost | null): Transport {
  const current = () => resolve(host ?? getHost());
  return {
    async unary(...args) {
      return current().unary(...args);
    },
    async stream(...args) {
      return current().stream(...args);
    },
  };
}

/** The late-binding transport `createFactoryClient()` uses by default. */
export const clientTransport: Transport = transportFor();
