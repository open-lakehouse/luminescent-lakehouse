// mangrove's Unity Catalog browser in the shell. Its client fetches through the
// host's `unity-catalog` binding (src/integrations/bind.ts), so the same view
// works against a local instance's gateway and a Neon branch's function.
//
// The explorer keeps its selection in the URL (`/catalog?sel=…`), so it runs on
// its own memory-history router: the shell has no URL of its own.

import { useEnvironmentKey } from "@open-lakehouse/host/react";
import { TooltipProvider } from "@open-lakehouse/ui-kit";
import {
  createUnityCatalogClient,
  prefetchCatalogs,
  UnityCatalogProvider,
} from "@open-lakehouse/unity-catalog-client/react";
import {
  CatalogExplorer,
  EnvironmentScopeProvider,
  ExternalDataPage,
  type StorageKind,
} from "@open-lakehouse/unity-catalog-ui";
import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import {
  createMemoryHistory,
  createRootRouteWithContext,
  createRoute,
  createRouter,
  Outlet,
  RouterProvider,
  useNavigate,
} from "@tanstack/react-router";
import { useState } from "react";
import { UC_API_ROOT } from "../services";

interface RouterContext {
  queryClient: QueryClient;
}

const rootRoute = createRootRouteWithContext<RouterContext>()({
  component: () => (
    <div className="h-full min-h-0">
      <Outlet />
    </div>
  ),
});

interface CatalogSearch {
  sel?: string;
  tab?: string;
}

// CatalogExplorer reads `sel` and `tab` from the route `/catalog`.
const catalogRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "catalog",
  validateSearch: (search: Record<string, unknown>): CatalogSearch => ({
    sel: typeof search.sel === "string" ? search.sel : undefined,
    tab: typeof search.tab === "string" ? search.tab : undefined,
  }),
  loader: ({ context }) => prefetchCatalogs(context.queryClient),
  component: CatalogExplorer,
});

const STORAGE_KINDS: StorageKind[] = ["external_location", "credential"];

// The tree's storage settings link here.
const externalDataRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "external-data",
  validateSearch: (search: Record<string, unknown>): { kind: StorageKind } => ({
    kind: STORAGE_KINDS.includes(search.kind as StorageKind)
      ? (search.kind as StorageKind)
      : "external_location",
  }),
  component: ExternalData,
});

function ExternalData() {
  const { kind } = externalDataRoute.useSearch();
  const navigate = useNavigate();
  return (
    <ExternalDataPage
      kind={kind}
      onKindChange={(next) => navigate({ to: "/external-data", search: { kind: next } })}
    />
  );
}

const routeTree = rootRoute.addChildren([catalogRoute, externalDataRoute]);

function createCatalogRouter(queryClient: QueryClient) {
  return createRouter({
    routeTree,
    context: { queryClient },
    history: createMemoryHistory({ initialEntries: ["/catalog"] }),
    defaultPreload: "intent",
  });
}

declare module "@tanstack/react-router" {
  interface Register {
    router: ReturnType<typeof createCatalogRouter>;
  }
}

/** The catalog browser for the current environment's `unity-catalog`. */
export function CatalogView() {
  const queryClient = useQueryClient();
  const environmentKey = useEnvironmentKey();
  // One client and router per mount: the view remounts on an environment switch.
  const [client] = useState(() => createUnityCatalogClient({ baseUrl: UC_API_ROOT }));
  const [router] = useState(() => createCatalogRouter(queryClient));
  return (
    <UnityCatalogProvider client={client}>
      <EnvironmentScopeProvider scopeId={environmentKey}>
        <TooltipProvider delayDuration={300}>
          <div className="h-full" data-testid="catalog-view">
            <RouterProvider router={router} />
          </div>
        </TooltipProvider>
      </EnvironmentScopeProvider>
    </UnityCatalogProvider>
  );
}
