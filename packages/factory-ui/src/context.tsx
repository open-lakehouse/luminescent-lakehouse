// The factory client a component tree uses. By default it is the host-bound
// client (it resolves the host's `factory` binding on every call); a story or
// a test passes its own.

import {
  createFactoryClient,
  type FactoryClient,
  transportFor,
} from "@open-lakehouse/factory-client";
import { useOptionalHost } from "@open-lakehouse/host/react";
import { createContext, type ReactNode, useContext, useMemo } from "react";

const ClientContext = createContext<FactoryClient | null>(null);

export interface FactoryClientProviderProps {
  /** The client to use; defaults to one bound to the surrounding host. */
  client?: FactoryClient;
  children?: ReactNode;
}

/** Provide a factory client to the components below. */
export function FactoryClientProvider({ client, children }: FactoryClientProviderProps) {
  const host = useOptionalHost();
  const value = useMemo(() => client ?? createFactoryClient(transportFor(host)), [client, host]);
  return <ClientContext.Provider value={value}>{children}</ClientContext.Provider>;
}

/** The provided client, else one bound to the surrounding (or registered) host. */
export function useFactoryClient(): FactoryClient {
  const provided = useContext(ClientContext);
  const host = useOptionalHost();
  return useMemo(() => provided ?? createFactoryClient(transportFor(host)), [provided, host]);
}
