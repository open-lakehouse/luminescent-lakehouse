// The control room as a standalone app. The host is registered before the UI
// renders, so the factory client resolves its binding.

import { FactoryControlRoom } from "@open-lakehouse/factory-ui";
import { registerHost } from "@open-lakehouse/host";
import { HostProvider } from "@open-lakehouse/host/react";
import { ThemeProvider, Toaster, TooltipProvider } from "@open-lakehouse/ui-kit";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createWebHost } from "./web-host";
import "./globals.css";

const host = createWebHost(import.meta.env.VITE_FACTORY_API ?? "");
registerHost(host);

const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false } },
});

const root = document.getElementById("root");
if (!root) throw new Error("missing #root");
createRoot(root).render(
  <StrictMode>
    <ThemeProvider>
      <TooltipProvider>
        <QueryClientProvider client={queryClient}>
          <HostProvider host={host}>
            <FactoryControlRoom />
          </HostProvider>
        </QueryClientProvider>
      </TooltipProvider>
      <Toaster />
    </ThemeProvider>
  </StrictMode>,
);
