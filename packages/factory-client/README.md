# @open-lakehouse/factory-client

The generated ConnectRPC client and types for the agentic factory's
`factory.v1` API (protobuf-es + connect-es), bound through the
`@open-lakehouse/host` contract, and an in-memory fake service for tests and
Storybook (`@open-lakehouse/factory-client/testing`).

Released as a tarball with `@open-lakehouse/factory-ui`, at the same version,
on [luminescent-lakehouse's GitHub Releases](https://github.com/open-lakehouse/luminescent-lakehouse/releases)
(`factory-v<version>`). [docs/factory/packages.md](../../docs/factory/packages.md)
covers how to pin it.

```ts
import { createFactoryClient } from "@open-lakehouse/factory-client";

// With no transport, every call resolves the host's `factory` binding.
const client = createFactoryClient();
const { graphs } = await client.listGraphs({});
```

The code under `src/gen/` is generated from `crates/factory-proto/proto`
(`just factory-gen`). Don't edit it by hand.
