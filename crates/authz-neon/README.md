# authz-neon

The lakehouse's Cedar policy decision point (PDP), packaged as a
[Neon Function](https://neon.com/docs/compute/functions/overview) with slug
`authz`. The other functions on a branch ask it whether a principal may perform
an action on a resource.

The PDP itself is breakwater's [`olai-pdp`](https://github.com/open-lakehouse/breakwater/tree/main/crates/pdp),
which also ships as the `ghcr.io/open-lakehouse/pdp` image. Its
[docs/pdp.md](https://github.com/open-lakehouse/breakwater/blob/main/docs/pdp.md)
covers the API, the entity model and the configuration. This crate hosts it on
the branch, the way the other `-neon` crates host their upstreams. It has two
parts:

- a Node-API addon, `src/lib.rs`, that starts `olai_pdp::embed` with the
  branch's configuration and drives its router in-process
- a short fetch handler, `function/index.ts`

It has no background work, so an isolate can be evicted at any time.

## Build and test

```bash
just build authz        # linux-arm64 bundle → crates/authz-neon/authz-neon.zip (size-gated)
just test authz         # host-native addon, end to end against the seeded local Postgres
just test-arm64 authz   # the deployable bundle, in node:24-bookworm-slim
```

The tests run against `luminescent-migrate authz` and `function/seed.sql`. The
seed holds the fixture users, service accounts, groups and grants, plus a
stand-in `neon_auth."user"`. These tests check this deployment's
configuration and policies. olai-pdp's own Rust tests cover the PDP.

- `smoke.test.mjs`: caller secrets, the namespace rule, batches, failing
  closed, and a Neon Console change counting from the next request. That last
  test needs `TEST_PSQL`, so `test-arm64` skips it.
- `fixtures.test.mjs`: the decision fixtures in `function/policies/tests/`.
  Every action of the composed schema needs at least one allow and one deny.
- `loader-error.test.mjs`: an unreachable database is a deny.
- `invalid-policy.test.mjs`: a policy that doesn't validate keeps the PDP from
  starting.

At `release-neon` (opt-level 3) the zip is 3.9 MiB of Neon's 10 MiB (8.7 MiB
unzipped), measured 2026-11-21.

## Configuration

`start` takes olai-pdp's configuration from the environment (`DATABASE_URL`,
`AUTHZ_ENTITY_TTL_MS`, and so on) and sets the rest for the branch:

- **Callers:** each lakehouse service, with its `AUTHZ_TOKEN_<SERVICE>` secret
  (see [docs/deployment.md](../../docs/deployment.md#secrets)), asking about
  its own namespace:

  | Secret | Namespace |
  |---|---|
  | `AUTHZ_TOKEN_UNITYCATALOG` | `UnityCatalog` |
  | `AUTHZ_TOKEN_HEADWATERS` | `Headwaters` |
  | `AUTHZ_TOKEN_OPENSHARING` | `OpenSharing` |

- **Users:** from Neon Auth's `neon_auth."user"`. A Console "Make admin" or a
  ban counts from the next request.
- **Policies:** olai-pdp's built-in `Lakehouse` schema and baseline (which
  forbids banned users and disabled service accounts), then
  `function/policies/`.

If the PDP can't start, for example because a policy doesn't validate, every
call gets a 503, and the caller denies. Decisions are logged at target
`olai_pdp::decision`.

The PDP connects with the function's `DATABASE_URL`, which is `neondb_owner`
for now. It reads only what the `authz_reader` role may read, so moving it to a
least-privilege login role in `authz_reader` (#26) changes nothing else.

## Policies

`function/policies/` holds the lakehouse's policies:

- `lakehouse.cedar`: `admin`. A Neon Auth admin may do anything except receive
  Unity Catalog's storage credentials, which take an EXTERNAL USE grant.
- `unitycatalog.cedar`: what each Unity Catalog privilege allows for each
  operation, following Databricks ([docs/auth.md](../../docs/auth.md#unity-catalog)).
- `tests/*.cases.mjs`: the decision fixtures. `fixtures.test.mjs` adds a sweep
  of every Unity Catalog action.

Each upstream owns its schema fragment. `scripts/authz-policies.sh` stages each
fragment into `dist/policies/` from the source cargo resolved for the pinned
rev, so a pin bump brings its fragment along. Today that is mangrove's
`UnityCatalog` namespace, in four fragments:

- its entities (`cedar/UnityCatalog.cedarschema`, from `olai-uc-server`)
- its Delta API's actions (`cedar/DeltaApi.cedarschema`, from `olai-uc-server`)
- its storage proxy's actions (`cedar/StorageProxy.cedarschema`, from
  `olai-uc-server`)
- its API's actions, one per operation, generated into `olai-uc-handlers`
  (`src/codegen/authz.cedarschema`)

`MANGROVE_ROOT=<checkout>` stages them from a local mangrove checkout instead.
headwaters and the sharing server join when they publish their fragments. The
staged copies also land next to the sources, gitignored, for editors and
`cedar validate`.

To try a policy change against an unmerged olai-pdp, patch the `olai-pdp` git
source from a local breakwater checkout in a gitignored `.cargo/config.toml`,
and never commit the patch.
