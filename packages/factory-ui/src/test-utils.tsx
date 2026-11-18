// Render helpers: a component under a fake host whose `factory` binding is the
// in-memory fake service, the way the desktop and Storybook bind it.

import { createFakeFactory, type FakeFactoryHandle } from "@open-lakehouse/factory-client/testing";
import { HostProvider } from "@open-lakehouse/host/react";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render } from "@testing-library/react";
import type { ReactNode } from "react";

export function renderWithFactory(
  ui: (h: FakeFactoryHandle) => ReactNode,
  seed: "demo" | undefined = "demo",
) {
  const handle = createFakeFactory({ seed });
  const host = createFakeHost({ services: { factory: { transport: handle.transport } } });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const result = render(
    <QueryClientProvider client={qc}>
      <HostProvider host={host}>{ui(handle)}</HostProvider>
    </QueryClientProvider>,
  );
  return { ...result, ...handle, host };
}
