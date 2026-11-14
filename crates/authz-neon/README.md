# authz-neon

The lakehouse's Cedar policy decision point (PDP), packaged as a
[Neon Function](https://neon.com/docs/compute/functions/overview) with slug
`authz`. The other functions on a branch ask it whether a principal may perform
an action on a resource. It has two parts:

- a Node-API addon, `src/lib.rs`, that runs olai-auth's `CedarAuthorizer` over
  the policies in `function/policies/` and the entities it loads from the
  branch's Postgres
- a short fetch handler, `function/index.ts`

It has no background work, so an isolate can be evicted at any time.

## Build and test

```bash
just build authz        # linux-arm64 bundle → crates/authz-neon/authz-neon.zip (size-gated)
just test authz         # host-native addon, end to end against the seeded local Postgres
just test-arm64 authz   # the deployable bundle, in node:24-bookworm-slim
```

The tests run against `luminescent-migrate authz` and `function/seed.sql`, which
holds the fixture users, service accounts, groups and grants, plus a stand-in
`neon_auth."user"`:

- `smoke.test.mjs`: caller secrets, the namespace rule, batches, failing
  closed, and a Neon Console change counting from the next request. That last
  test needs `TEST_PSQL`, so `test-arm64` skips it.
- `fixtures.test.mjs`: the decision fixtures in `function/policies/tests/*.json`.
  Every action of the composed schema needs at least one allow and one deny.
- `loader-error.test.mjs`: an unreachable database is a deny.
- `invalid-policy.test.mjs`: a policy that doesn't validate keeps the PDP from
  starting.

At `release-neon` (opt-level 3) the zip is 3.6 MiB of Neon's 10 MiB (8.1 MiB
unzipped), measured 2026-11-14.

## API

The wire format is olai-auth's `wire` module. The services call it through
`olai_auth::HttpAuthorizer`, which fails closed and times out after 2 s.

| Endpoint | |
|---|---|
| `GET /health` | Open. `OK` |
| `POST /v1/authorize` | An `AuthorizeRequest`, answered with an `AuthorizeResponse` (`decision`, the determining `policies` by `@id`, `errors`) |
| `POST /v1/authorize/batch` | `{"requests": […]}`, answered in order with `{"responses": […]}`. At most 1000 requests per batch. |

Callers authenticate with `Authorization: Bearer <secret>`, where the secret is
the value of their `AUTHZ_TOKEN_<SERVICE>` (see
[docs/deployment.md](../../docs/deployment.md#secrets)). A wrong or missing
secret gets a 401. A caller may only ask about actions in its own Cedar
namespace, and anything else gets a 403, which refuses the whole batch:

| Secret | Namespace |
|---|---|
| `AUTHZ_TOKEN_UNITYCATALOG` | `UnityCatalog` |
| `AUTHZ_TOKEN_HEADWATERS` | `Headwaters` |
| `AUTHZ_TOKEN_OPENSHARING` | `OpenSharing` |

A request whose decision can't be made gets a 200 with a deny and the reason
in `errors`. That covers an unknown principal, a database error, and entities
or a context that don't match the schema. If the PDP can't start, for example
because a policy doesn't validate, every call gets a 503. Either way the caller
denies.

Each decision is logged (target `authz_neon::decision`) with the caller, the
principal, the action, the resource, the decision, the determining policies and
any errors.

## Policies

`function/policies/` holds:

- `Lakehouse.cedarschema`, the principals every service's fragment refers to:
  `User`, `ServiceAccount`, `Group`, `Role`
- `baseline.cedar`, which holds in every namespace:
  - `admin`: a Neon Auth admin may do anything except receive Unity Catalog's
    storage credentials (that takes an EXTERNAL USE grant)
  - `banned`: forbids banned users
  - `disabled`: forbids disabled service accounts
- `unitycatalog.cedar`: what Unity Catalog's privileges allow each operation,
  after Databricks ([docs/auth.md](../../docs/auth.md#unity-catalog))
- `tests/*.json`, `tests/*.cases.mjs`: the decision fixtures (`fixtures.test.mjs`
  adds a sweep of every Unity Catalog action)

Each upstream owns its schema fragment. `scripts/authz-policies.sh` stages it
into `dist/policies/` from the source cargo resolved for the pinned rev, so a
pin bump brings the fragment with it. Today that is mangrove's `UnityCatalog`
namespace, in four fragments: its entities (`cedar/UnityCatalog.cedarschema`)
and the actions of its Delta API and storage proxy (`cedar/DeltaApi.cedarschema`,
`cedar/StorageProxy.cedarschema`) from
`olai-uc-server`, and its API's actions, one per operation, generated into
`olai-uc-handlers` (`src/codegen/authz.cedarschema`). `MANGROVE_ROOT=<checkout>`
stages them from a local mangrove checkout instead. headwaters and the sharing
server join when they publish theirs. The staged copies also land next to the
sources, gitignored, for editors and `cedar validate`.

At start, the addon merges every `*.cedarschema` fragment, namespace by
namespace (a namespace may span fragments), concatenates every `*.cedar` file,
and validates the policies in strict mode.

## Entities

`src/loader.rs` builds a request's entities from the branch's database:

- **`Lakehouse::User`** comes from `neon_auth."user"`:
  - `neon_role`: `role`, defaulting to `user`
  - `banned`: while `banExpires` hasn't passed
  - `email_verified`

  Token claims are never used: the JWT `role` claim is the Data API role.
- **`Lakehouse::ServiceAccount`** comes from `authz.service_accounts` (`name`,
  `disabled`).
- **Parents:** the principal's groups (`authz.memberships`, transitively) as
  `Lakehouse::Group::"<name>"`, and a user's Neon Auth role as
  `Lakehouse::Role::"<role>"`.
- **Grants** (olai-auth's `GrantsEntityLoader` over `authz.grants`): on the
  request's resource and every entity the caller sent with it (Unity Catalog
  sends the securable's ancestors), what the principal holds, directly or
  through a group:
  - `privileges`: granted on that entity itself (and `OWNER`);
  - `inherited_privileges`: on it or any of its ancestors.

  Cedar can't quantify over a set of grants, so the loader resolves them for
  the request at hand. A policy reads, for example,
  `resource.schema.privileges.contains("USE_SCHEMA")`, or
  `resource.inherited_privileges.contains("SELECT")` for a grant on the table
  or above it. A caller that sends either attribute itself is refused.

The PDP never reads a service's own tables. Their structure comes with the
request, in `entities`.

The principal's row is read on every request, so a Console "Make admin", a ban
or a disabled account counts from the next request. Groups and grants are
cached for `AUTHZ_ENTITY_TTL_MS`, 2000 by default and 0 for off. A new or
revoked grant or membership therefore takes up to that long to count. A
caller's `CachedAuthorizer` adds its own TTL on top.

The PDP connects with the function's `DATABASE_URL`, which is `neondb_owner`
for now. It reads only what the `authz_reader` role may read, so moving it to a
least-privilege login role in `authz_reader` (#26) changes nothing else.
Loading times out after 1.5 s, under the callers' 2 s.
