// Every story renders under a fake host whose `factory` binding is the
// in-memory fake service seeded with the demo plan, the same binding path the
// desktop and the web app use. A story shapes the fake before it renders with
// `parameters.factory.setup` (a queue, say).

import { createFakeFactory, type FakeFactory } from "@open-lakehouse/factory-client/testing";
import { HostProvider } from "@open-lakehouse/host/react";
import { createFakeHost } from "@open-lakehouse/host/testing";
import { ThemeProvider, TooltipProvider } from "@open-lakehouse/ui-kit";
import type { Decorator, Preview } from "@storybook/react-vite";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useState } from "react";
import "../src/globals.css";

const withFactory: Decorator = (Story, context) => {
  const [env] = useState(() => {
    const factory = createFakeFactory({ seed: "demo" });
    const setup = context.parameters.factory?.setup as ((fake: FakeFactory) => void) | undefined;
    setup?.(factory.fake);
    return {
      factory,
      host: createFakeHost({ services: { factory: { transport: factory.transport } } }),
      queryClient: new QueryClient({ defaultOptions: { queries: { retry: false } } }),
    };
  });
  return (
    <ThemeProvider>
      <TooltipProvider>
        <QueryClientProvider client={env.queryClient}>
          <HostProvider host={env.host}>
            <div style={{ height: "100vh" }}>
              <Story />
            </div>
          </HostProvider>
        </QueryClientProvider>
      </TooltipProvider>
    </ThemeProvider>
  );
};

const preview: Preview = {
  decorators: [withFactory],
  parameters: { layout: "fullscreen" },
};

export default preview;
