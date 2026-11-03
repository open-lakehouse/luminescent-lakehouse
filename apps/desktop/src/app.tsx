// The app root. Imported by main.ts only after the host is registered, so
// anything that resolves the host at import time sees the desktop's.

import type { LakehouseHost } from "@open-lakehouse/host";
import { HostProvider } from "@open-lakehouse/host/react";
import { ThemeProvider, Toaster } from "@open-lakehouse/ui-kit";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { type ReactNode, StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { ShellLayout } from "./shell/ShellLayout";
import { type ShellHost, ShellHostProvider } from "./shell/shell-host";
import "./globals.css";

export function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false } },
  });
}

export interface ProvidersProps {
  host: LakehouseHost;
  shell: ShellHost;
  queryClient?: QueryClient;
  children: ReactNode;
}

/** Every provider the shell's views need; tests wrap views in it with fakes. */
export function Providers({ host, shell, queryClient, children }: ProvidersProps) {
  return (
    <ThemeProvider>
      <QueryClientProvider client={queryClient ?? createQueryClient()}>
        <HostProvider host={host}>
          <ShellHostProvider host={shell}>{children}</ShellHostProvider>
        </HostProvider>
      </QueryClientProvider>
      <Toaster />
    </ThemeProvider>
  );
}

export function mount(root: HTMLElement, host: LakehouseHost, shell: ShellHost) {
  const queryClient = createQueryClient();
  createRoot(root).render(
    <StrictMode>
      <Providers host={host} shell={shell} queryClient={queryClient}>
        <ShellLayout />
      </Providers>
    </StrictMode>,
  );
}
