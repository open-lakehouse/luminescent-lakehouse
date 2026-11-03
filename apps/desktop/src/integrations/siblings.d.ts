// The surface of mangrove's and headwaters' UI packages that the shell uses.
// Vite builds the packages from source at pinned commits
// (sibling-uis.ts), but tsc doesn't check them from here: they resolve their
// own copies of React Query, ConnectRPC and protobuf, whose types don't unify
// with this app's. These declarations go when the packages are published and
// pinned like the trestle ones (docs/desktop.md).

declare module "@open-lakehouse/unity-catalog-client" {
  export function setDefaultUnityCatalogFetch(fetch: typeof globalThis.fetch): void;
}

declare module "@open-lakehouse/unity-catalog-client/react" {
  import type { QueryClient } from "@tanstack/react-query";
  import type { ReactNode } from "react";

  export interface UnityCatalogClient {
    readonly $api: unknown;
    readonly fetchClient: unknown;
  }
  export function createUnityCatalogClient(opts?: {
    baseUrl?: string;
    fetch?: typeof globalThis.fetch;
  }): UnityCatalogClient;
  export function UnityCatalogProvider(props: {
    client: UnityCatalogClient;
    children: ReactNode;
  }): ReactNode;
  export function prefetchCatalogs(queryClient: QueryClient): Promise<unknown>;
}

declare module "@open-lakehouse/unity-catalog-ui" {
  import type { ReactNode } from "react";

  export type StorageKind = "external_location" | "credential";
  export function CatalogExplorer(): ReactNode;
  export function ExternalDataPage(props: {
    kind: StorageKind;
    onKindChange: (kind: StorageKind) => void;
  }): ReactNode;
  export function EnvironmentScopeProvider(props: {
    scopeId: string;
    children: ReactNode;
  }): ReactNode;
}

declare module "@headwaters/lineage-client" {
  export function registerTransport(transport: import("@connectrpc/connect").Transport): void;
}

declare module "@headwaters/lineage-ui" {
  import type { ReactNode } from "react";

  export interface LineageNode {
    id: string;
    data?: unknown;
  }
  type Select = (entity: { namespace: string; name: string }) => void;
  interface DetailProps {
    namespace: string;
    name: string;
    onViewLineage?: (nodeId: string) => void;
    onClose?: () => void;
  }

  export function ReadClientProvider(props: { children: ReactNode }): ReactNode;
  export function HomeView(): ReactNode;
  export function DatasetBrowser(props: {
    namespace?: string;
    limit?: number;
    onSelect?: Select;
  }): ReactNode;
  export function JobBrowser(props: {
    namespace?: string;
    limit?: number;
    onSelect?: Select;
  }): ReactNode;
  export function SearchView(props: {
    query: string;
    onQueryChange: (query: string) => void;
    onSelect?: (result: { nodeId: string; type: string }) => void;
  }): ReactNode;
  export function LineageView(props: {
    nodeId: string;
    selectedId?: string;
    onSelect?: (node: LineageNode) => void;
  }): ReactNode;
  export function DatasetDetailPanel(props: DetailProps): ReactNode;
  export function JobDetailPanel(props: DetailProps): ReactNode;
}
