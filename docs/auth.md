# Authentication

Callers authenticate with [Neon Auth](https://neon.com/docs/auth/overview)
(Managed Better Auth). Machines will use API keys and GitHub Actions OIDC
tokens instead. The design and the remaining work are tracked in
[#4](https://github.com/open-lakehouse/luminescent-lakehouse/issues/4); this
page is the runbook for what is in place.

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

The resource of each function is in `authSetup` in [`neon.ts`](../neon.ts).
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

The OAuth server stays off for now. Neon only enables it with a login page
and a consent page to send users to, and those will live in the web app. A
child branch inherits its parent's clients, ids included, so the CLI has one
client id per environment.

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
- Replace or remove Neon's shared Google app, which is meant for development.
- A custom SMTP provider (`neon neon-auth config email-provider`), or turn off
  email and password sign-in if GitHub is enough
  (`neon neon-auth config email-password update --enabled false`).
- Decide whether to keep the organization plugin, which is on by default.

See Neon's [production checklist](https://neon.com/docs/auth/production-checklist).
