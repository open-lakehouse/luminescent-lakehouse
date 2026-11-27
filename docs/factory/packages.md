# The factory's npm packages

`@open-lakehouse/factory-client` (the factory.v1 client and its in-memory
fake) and `@open-lakehouse/factory-ui` (the control room and its parts) are
released as tarballs on this repository's GitHub Releases, the way trestle
ships ui-kit and host. Nothing goes to npm. A host outside this repository,
such as olai-sigint's Factory pages, pins them by version. The desktop app and
`apps/factory` keep building them from source in the workspace.

## Releases

The packages have a release-plz stream of their own:

- **The placeholder** is [`packages/Cargo.toml`](../../packages/Cargo.toml)
  (`luminescent-factory-packages`). Its version is the version of both
  packages, which are always released together. It sits in `packages/` rather
  than under `crates/` because release-plz 0.4.3 only walks the history under a
  package's own directory. The desktop's symlinked placeholder can't see its
  app's changes for that reason (#235). Its `include` lists what the tarballs
  are built from: both packages' manifests and sources, `tsconfig.base.json`
  and `packages/scripts/`. Plan review's view isn't part of it, because it
  ships inside workgraph-mcp.
- **The config** is
  [`release-plz-factory.toml`](../../release-plz-factory.toml). Tags are
  `factory-v<version>`, and the changelog is
  [`packages/CHANGELOG.md`](../../packages/CHANGELOG.md). The release PR is
  `chore(factory): release v<version>` on a `factory-release-plz-*` branch.
  The release starts as a draft and is never marked latest, since GitHub's
  latest release belongs to the desktop. Commits that change the packages
  count toward a release, so scope them `factory` or `factory-ui`.
- **The workflow** is
  [`factory-release.yml`](../../.github/workflows/factory-release.yml). On a
  push to main it tags any untagged version, which is what merging the release
  PR does. It then packs that tag (`just factory-pack`), attests the tarballs'
  build provenance, uploads them with `SHA256SUMS` to the draft and publishes
  it. To retry, run it by hand with the tag. `factory.yml` packs on every pull
  request that touches the packages, as a dry run.

`just factory-pack [version]` writes
`target/release-assets/open-lakehouse-factory-{client,ui}-<version>.tgz`.
[`packages/scripts/pack.ts`](../../packages/scripts/pack.ts) builds each
package to `dist/` with `tsc` (one module per source file, no bundler) and
checks that every bare import in `dist/` is declared. It then stamps the
manifest for consumers, packs it with `bun pm pack` and puts the manifest back.
Stamping does four things:

- It sets the release version (`package.json` stays at `0.0.0` in git).
- It pins the factory-client dependency to that version.
- It points `exports` at `dist/`.
- It turns factory-client and `@tanstack/react-query` into factory-ui
  **peers**. The consumer provides both, because a second copy of either
  splits their context.

## Pinning them

Add both tarballs at the same version, plus factory-ui's peers. List the
`@open-lakehouse/*` tarballs again under `overrides`. factory-ui names its
peers by version (`@open-lakehouse/factory-client@0.0.1`), and none of them
is on npm, so without an override the package manager looks for them in the
registry and fails. This repository's root `package.json` does the same for
host and ui-kit.

```jsonc
// package.json
"dependencies": {
  "@open-lakehouse/factory-client": "https://github.com/open-lakehouse/luminescent-lakehouse/releases/download/factory-v0.0.1/open-lakehouse-factory-client-0.0.1.tgz",
  "@open-lakehouse/factory-ui": "https://github.com/open-lakehouse/luminescent-lakehouse/releases/download/factory-v0.0.1/open-lakehouse-factory-ui-0.0.1.tgz",
  // factory-ui's peers, at the versions its package.json names:
  "@open-lakehouse/host": "https://github.com/open-lakehouse/trestle/releases/download/host-v0.0.2/open-lakehouse-host-0.0.2.tgz",
  "@open-lakehouse/ui-kit": "https://github.com/open-lakehouse/trestle/releases/download/ui-kit-v0.0.3/open-lakehouse-ui-kit-0.0.3.tgz",
  "@open-lakehouse/editor": "https://github.com/open-lakehouse/mangrove/releases/download/editor-v0.0.2/open-lakehouse-editor-0.0.2.tgz",
  "@tanstack/react-query": "^5.105.0",
  "react": "^19.0.0",
  "react-dom": "^19.0.0"
},
"overrides": {
  "@open-lakehouse/factory-client": "https://github.com/open-lakehouse/luminescent-lakehouse/releases/download/factory-v0.0.1/open-lakehouse-factory-client-0.0.1.tgz",
  "@open-lakehouse/factory-ui": "https://github.com/open-lakehouse/luminescent-lakehouse/releases/download/factory-v0.0.1/open-lakehouse-factory-ui-0.0.1.tgz",
  "@open-lakehouse/host": "https://github.com/open-lakehouse/trestle/releases/download/host-v0.0.2/open-lakehouse-host-0.0.2.tgz",
  "@open-lakehouse/ui-kit": "https://github.com/open-lakehouse/trestle/releases/download/ui-kit-v0.0.3/open-lakehouse-ui-kit-0.0.3.tgz",
  "@open-lakehouse/editor": "https://github.com/open-lakehouse/mangrove/releases/download/editor-v0.0.2/open-lakehouse-editor-0.0.2.tgz"
}
```

To upgrade, change the version in every factory URL together. factory-ui's peer on
factory-client is exact. Each release's notes list its changes.

The host then binds the factory and renders the control room inside a
`QueryClientProvider` and the host's `HostProvider`, as
[`apps/factory/src/main.tsx`](../../apps/factory/src/main.tsx) does. The
`factory` service binding is any `fetch` that reaches factory.v1's Connect
routes (`/factory.v1.FactoryService/<Method>`), as in
[`web-host.ts`](../../apps/factory/src/web-host.ts), or a Connect `transport`.

### Bundler

The packages assume Vite, as ui-kit and editor do.
[`apps/factory/vite.config.ts`](../../apps/factory/vite.config.ts) is the
reference:

- `resolve.dedupe` covers react, react-dom, `@tanstack/react-query`,
  `@open-lakehouse/{ui-kit,host,editor}`, `monaco-editor`, `@xyflow/react` and
  `@connectrpc/connect{,-web}`.
- editor's `dist/` imports Monaco's workers with Vite's `?worker` suffix. The
  dependency optimizer needs the `external-workers` plugin from that config.
- Tailwind 4 skips `node_modules`, so register the packages' classes:
  `@source "../node_modules/@open-lakehouse/factory-ui/dist";` (and ui-kit's
  and editor's). Import `@xyflow/react/dist/style.css` too: the canvas's
  styles are the host's job.

**Monaco loads only when it's used.** Every Monaco view (the markdown editor,
the text diff, the node diff) is behind a dynamic `import()`, which stays a
module of its own in `dist/`, so the host's bundler puts Monaco in a separate
chunk. The inspector shows a plain text field until someone edits it, and
never loads the editor when it's read-only. The tree diff and the canvas never
load Monaco. A test in factory-ui (`monaco/split.test.ts`) fails if a static
import reaches Monaco.

### Streaming through a proxy

`useWatchGraph`, which `FactoryControlRoom` uses, holds a WatchGraph
server-streaming call open for the plan on screen. A proxy in front of
factory-server must stream it: pass Connect's streaming responses through
without buffering, and don't time out an idle stream sooner than a few
minutes.

Streams will still end, for example when the proxy or the App restarts. The
hook then reconnects with backoff (from 0.5 s, doubling, up to 30 s) and
refetches the graph, its runs and the places (ListPlaces) on the first message
after each reconnect, so a change made while it was away still shows. While it's
reconnecting, the control room says so.

### Queued runs

A run waiting for a place in a capped environment (`Environment.max_sessions`,
[hosts.md](hosts.md#waiting-for-a-place)) carries `Run.waiting`. Its card reads
`queued · <environment> · 2nd of 3`, and the inspector says since when and how
many places are taken. The line is first come, first served across every
graph, so the place comes from `ListPlaces` (`usePlaces`), not from one graph's
runs. The control room fetches it only while a run on screen waits, and polls
it then, since runs in other graphs move the line.

