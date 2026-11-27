# @open-lakehouse/factory-ui

React components for the agentic factory's control room:
`FactoryControlRoom`, and its parts `PlanCanvas`, `NodeInspector`,
`RevisionTimeline`, `PlanDiffView` and `NodeDiffPanel`, with react-query hooks
over factory.v1 (`useGraph`, `useRuns`, `useWatchGraph`, …). They reach the
factory through `@open-lakehouse/factory-client` and the host's `factory`
binding, and are styled by `@open-lakehouse/ui-kit`'s tokens.

Released as a tarball with `@open-lakehouse/factory-client`, at the same
version, on [luminescent-lakehouse's GitHub Releases](https://github.com/open-lakehouse/luminescent-lakehouse/releases)
(`factory-v<version>`). [docs/factory/packages.md](../../docs/factory/packages.md)
covers how to pin it, its peers, and what a Vite host needs (dedupe, Monaco's
workers, Tailwind's `@source`).

```tsx
import { FactoryControlRoom } from "@open-lakehouse/factory-ui";

<QueryClientProvider client={queryClient}>
  <HostProvider host={host}>
    <FactoryControlRoom />
  </HostProvider>
</QueryClientProvider>;
```
