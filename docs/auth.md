# Authentication

Callers authenticate with [Neon Auth](https://neon.com/docs/auth/overview)
(Managed Better Auth). Machines will use API keys and GitHub Actions OIDC
tokens instead. The design and the remaining work are tracked in
[#4](https://github.com/open-lakehouse/luminescent-lakehouse/issues/4); this
page is the runbook for what is in place.

Of the deployed functions, `unitycatalog` verifies its callers: OAuth access
tokens for its resource, the web app's session tokens, and API keys
([its README](../crates/unitycatalog-neon/README.md#runtime-configuration)).
The `authz` function decides what each caller may do there
([Unity Catalog](#unity-catalog)). headwaters is still anonymous.

## One identity provider per branch

Neon Auth is on for every environment branch. Each branch has its own Auth
URL and its own users, and a new branch starts with a copy of its parent's.
`neon neon-auth status --branch <branch>` prints the branch's `base_url` and
`jwks_url`, and a function on the branch gets them as `NEON_AUTH_BASE_URL` and
`NEON_AUTH_JWKS_URL`.

Branches can share a signing key: the preview branches sign with staging's.
So a staging token's signature verifies on a preview, and only an exact `iss`
check tells the branches apart.

What a verifier has to know about the tokens (all EdDSA, Ed25519):

| | Session token (web app, `authClient.token()`) | OAuth access token (CLI, MCP, partner apps) |
|---|---|---|
| `typ` header | none | `at+jwt` |
| Lifetime | 15 minutes | 1 hour; refresh tokens rotate on use |
| `iss` | the Auth URL's **origin** (`https://ep-….neonauth.…`) | the full Auth URL, `…/neondb/auth` |
| `aud` | the same origin, so not scoped to a service | an array: the requested `resource`, plus `<Auth URL>/oauth2/userinfo` |
| Other claims | `sub`, `email`, `role`, `banned`, … | `sub`, `client_id`, `azp`, `scope`, `sid` |

The resource of each function is in `TENANT.resources` in [`neon.ts`](../neon.ts).
The session token's `role` claim is the Data API's Postgres role
(`authenticated`), not the user's Neon Auth role, and OAuth tokens carry no
role at all. Never use a token to decide who is an admin.

## Admins

A user is an admin when `neon_auth."user".role` is `admin` on that branch.
Make one in the Neon Console (Auth → Users → "Make admin") or with the CLI:

```bash
bunx neon neon-auth user set-role <user-id> --branch <branch> --roles admin
```

The services read the role (and `banned`) from the database on each request,
so a change takes effect immediately and doesn't wait for a token to expire.

Users and roles are copied when a branch is created. Staging and previews
start with production's users, so a production admin is an admin there too.
Removing an admin on production doesn't reach branches that already exist:
remove them there as well.

## Permission data

Everything else about who may do what lives in the `authz` schema, next to
`neon_auth` in the branch's database. `luminescent-migrate authz` creates it,
and scripts/migrate.sh runs that first on every deploy. Its tables are
olai-auth's (`olai_auth::migrate`: service accounts, API keys, groups,
memberships, grants), plus the lakehouse's own (GitHub trust, the reader
role's access to Neon Auth). Each keeps its own migration ledger,
`authz._sqlx_migrations` and `luminescent._sqlx_migrations`, because
headwaters and the sharing server refuse to start when the unqualified
`_sqlx_migrations` holds migrations they don't know.

A branch whose `authz` predates olai-auth's migrations (its ledger holds the
lakehouse's old `0001_init`) is refused. Reset it, which drops every service
account, API key, trust rule, group and grant on the branch, then grant again:

```bash
DATABASE_URL=<the branch's direct connection string> luminescent-migrate authz --reset
```

| Table | Holds |
|---|---|
| `service_accounts` | Machine callers. A disabled account authenticates with nothing. |
| `api_keys` | Keys of service accounts: the id, the SHA-256 of the key, scopes, expiry, revocation |
| `github_oidc_trust` | Which GitHub Actions OIDC tokens act as a service account |
| `groups`, `memberships` | Groups of users, service accounts and groups |
| `grants` | A subject holds a privilege (`USE_SCHEMA`, `reader`, …) on a Cedar entity (`UnityCatalog::Schema` `main.sales`) |

Manage them with `just admin <branch> …` (`luminescent-admin`):

```bash
just admin staging service-account create airflow --description "lineage from Airflow"
just admin staging api-key issue airflow --scope ingest --expires-in-days 90   # prints the key once
just admin staging api-key revoke <id>
just admin staging github-trust add deploy --owner-id <id> --repository-id <id> --ref refs/heads/main
just admin staging group create data
just admin staging group add data user:<neon-auth-user-id>
just admin staging grant add group:data reader Headwaters::Namespace prod
```

Principals are written `user:<Neon Auth user id>`, `service-account:<name>`
or `group:<name>`. Like users, this data is copied when a branch is created.

API keys look like `olk_<id>_<secret>`: 16 and 64 lowercase hex characters.
Only the SHA-256 of the whole key string is stored, and the key is shown once,
when it is issued. GitHub trust rules match GitHub's numeric owner and
repository ids, never names: an owner or repository that is renamed, then
registered again by someone else, must not inherit the trust.
`gh api repos/<owner>/<repo> --jq '.owner.id, .id'` prints both.

### Authorization

The `authz` function ([crates/authz-neon](../crates/authz-neon/README.md)) is
the Cedar policy decision point that decides with this data. Its entity loader
(olai-auth's grants loader) turns the caller's groups into Cedar parents, and
puts the privileges it holds, itself or through a group, on the requested
securable and on each of its ancestors: `privileges` (granted on that one) and
`inherited_privileges` (on it or above it). So a policy can tell a grant on a
table from one on its schema. Admin and ban come from `neon_auth."user"` on
every request.

### Unity Catalog

Unity Catalog asks about every request: one Cedar action, the operation
(`GetTable`, `CreateTable`, `GenerateTemporaryTableCredentials`, `CommitTable`,
…), on one securable named by its full name, `UnityCatalog::Table`
`main.sales.orders`, sent with its containers. The privileges are Unity
Catalog's (its grants API, `/permissions/{type}/{name}`), and the policies
(`crates/authz-neon/function/policies/unitycatalog.cedar`) follow Databricks':

| Operation | Needs |
|---|---|
| anything in a catalog or schema | usage: `USE_CATALOG` on the catalog, `USE_SCHEMA` on the schema |
| `Get*` (metadata) | usage + a privilege on it (`SELECT`, `MODIFY`, `EXECUTE`, `READ_VOLUME`, `MANAGE`, …), or `BROWSE` |
| lists | on the metastore: anyone, filtered by `Get*`; in a catalog or schema: usage, or `BROWSE` |
| `CreateCatalog`, … | `CREATE_CATALOG` (`CREATE_STORAGE_CREDENTIAL`, `CREATE_EXTERNAL_LOCATION`, …) on the metastore |
| `CreateSchema` / `CreateTable`, … | usage + `CREATE_SCHEMA` on the catalog / `CREATE_TABLE`, `CREATE_VOLUME`, `CREATE_FUNCTION`, `CREATE_MODEL` on the schema |
| `Update*`, `Delete*` | usage + `MANAGE`, or owning it or a container |
| `CommitTable` (Delta) | usage + `SELECT` + `MODIFY` |
| table / volume / model-version credentials | usage + `SELECT` / `READ_VOLUME` / `EXECUTE` (and `MODIFY` / `WRITE_VOLUME` to write) + `EXTERNAL_USE_SCHEMA` on the schema |
| path credentials | `READ_FILES` (and `WRITE_FILES`) + `EXTERNAL_USE_LOCATION` on the external location |
| `GetGrants` / `UpdateGrants` | a privilege on it, `READ_METADATA`, `MANAGE` or owning / `MANAGE` or owning |

- **Grants are inherited downwards**: `SELECT` on a catalog covers its tables,
  `USE_SCHEMA` on a catalog its schemas. Never upwards, and `USE_SCHEMA` on a
  table is not `USE_SCHEMA` on its schema.
- **Owning** a securable (the server records it when it is created) is every
  privilege on it but EXTERNAL USE. Owning a container lets you use and manage
  what is in it, not read its data.
- **ALL_PRIVILEGES** is every privilege but `EXTERNAL_USE_SCHEMA`,
  `EXTERNAL_USE_LOCATION` and `MANAGE`: it never hands out storage keys nor
  the right to change grants.
- **Admins** may do anything but receive credentials, which takes an explicit
  EXTERNAL USE grant, as for everyone (then no data privilege is needed).

Grant through Unity Catalog's grants API (`PATCH /permissions/…`), or, to
bootstrap, with the admin CLI:

```bash
just admin staging grant add group:data USE_CATALOG UnityCatalog::Catalog main
just admin staging grant add group:data "USE SCHEMA" UnityCatalog::Schema main.sales
just admin staging grant add group:data SELECT UnityCatalog::Schema main.sales
just admin staging grant add service-account:etl EXTERNAL_USE_SCHEMA UnityCatalog::Schema main.sales
just admin staging grant add user:<id> OWNER UnityCatalog::Catalog main
```

`luminescent-admin` takes either spelling (`USE_SCHEMA`, or OSS's `USE SCHEMA`)
and stores the canonical one. It refuses an unknown privilege or securable on a
`UnityCatalog::` object, and an id that isn't its full name. Grants are keyed by
full name; Unity Catalog re-keys them when it renames a securable.

### The reader role

The migration creates `authz_reader`, a role that cannot log in. It holds
what the functions need to authenticate and authorize callers, and nothing
more:

- read access to `authz`;
- `id`, `role`, `banned`, `banExpires` and `emailVerified` of
  `neon_auth."user"`;
- `userId`, `providerId` and `accountId` of `neon_auth.account`.

It gets no emails, sessions, tokens or password hashes, and no write access.
For now the functions still connect as `neondb_owner`, the role Neon injects
into `DATABASE_URL`, which can write all of `neon_auth`. Moving them to a
login role that joins `authz_reader` is a follow-up.

## Branch settings

`neon.ts` can only switch Neon Auth on (`auth: true`). The rest is
`authSetup` in `neon.ts`, which
[`scripts/auth-setup.ts`](../scripts/auth-setup.ts) applies to the branch
after every `neon deploy` (`just deploy`, and CI's
[`neon-deploy.yml`](../.github/workflows/neon-deploy.yml)):

| Setting | What the script does |
|---|---|
| OAuth protected resources | Sets them to exactly one per function in `neon.ts` |
| Trusted domains | Adds the missing ones; reports, never removes, the others |
| OAuth server | Enables it with the login and consent pages from `oauthServer`; leaves it alone while `oauthServer` is unset |
| CLI client | Once the server is on, creates the public client (PKCE, loopback redirect) or updates its redirect URIs and resources |

Running it again changes nothing. By hand: `bun scripts/auth-setup.ts <branch>`.

The OAuth server is on for every branch, pointed at that branch's own pages
in the web app (see [web.md](web.md)):

```
https://luminescent-lakehouse.vercel.app/a/<auth-host>/<db>/auth/sign-in
https://luminescent-lakehouse.vercel.app/a/<auth-host>/<db>/auth/consent
```

`<auth-host>/<db>/auth` is the branch's Auth URL (`neon neon-auth status`)
without the scheme, and the origin is `TENANT.webOrigin` in `neon.ts`, which a
lakehouse deployed from another repository sets to its own
([web.md](web.md#self-hosting)). Neon only
enables the server with both pages set, and their origin has to be a trusted
domain, which `authSetup` also sets. A child branch starts with its parent's
pages, so the script repoints them on its first deploy. It inherits its
parent's clients, ids included, so the CLI has one client id per environment.

## Production checklist

These need credentials or a decision, so they are set once on production by
hand. Branches created afterwards inherit them. On production, the script
warns about the first two.

- Sign in with GitHub through our own GitHub OAuth app. Its callback is
  `<base_url>/callback/github`.
  ```bash
  bunx neon neon-auth oauth-provider add --branch production --provider-id github \
    --oauth-client-id <id> --oauth-client-secret <secret>
  ```
  Neon has no shared GitHub app, and a GitHub OAuth app takes one callback
  host while each branch's Auth URL has its own. So staging gets its own app
  and the same command with `--branch staging`. A branch inherits its
  parent's provider, but GitHub only redirects to the parent's callback, so
  GitHub sign-in doesn't work on preview branches.
- Replace or remove Neon's shared Google app, which is meant for development.
- A custom SMTP provider (`neon neon-auth config email-provider`), or turn off
  email and password sign-in if GitHub is enough
  (`neon neon-auth config email-password update --enabled false`).
- Decide whether to keep the organization plugin, which is on by default.

See Neon's [production checklist](https://neon.com/docs/auth/production-checklist).
