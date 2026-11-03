# Web app

[`apps/web`](../apps/web) holds the browser pages for Neon Auth: the OAuth
server's login and consent pages, and a page that signs in with GitHub and
shows who you are. It is a static Vite + React SPA (shadcn components, Tailwind 4, the docs-factory
theme), tracked in
[#33](https://github.com/open-lakehouse/luminescent-lakehouse/issues/33).

## One deployment for every branch

Every Neon branch has its own Neon Auth server (see [auth.md](auth.md)), so
its pages have to talk to that branch's Auth URL. Instead of one deployment
per branch, the branch comes from the path:

```
https://<app host>/b/<endpoint-id>/…
```

`<endpoint-id>` is the branch's compute endpoint (`ep-…`), the first label of
its Auth URL's host. The app rebuilds the Auth URL from it and two settings
that every branch of the project shares:

```
https://<endpoint-id>.<VITE_NEON_AUTH_HOST_SUFFIX>/<VITE_NEON_AUTH_DB>/auth
```

So a new preview branch works as soon as it exists, without a redeploy. A path
that isn't `/b/ep-[a-z0-9-]+/…` renders the 404 page.

## Pages

| Path | What |
|---|---|
| `/b/<endpoint-id>/` | Who's signed in on that branch; GitHub sign-in and sign-out. For checking a branch by hand. |
| `/b/<endpoint-id>/auth/sign-in` | The OAuth server's login page. |
| `/b/<endpoint-id>/auth/consent` | The OAuth server's consent page. |

Each branch's OAuth server is pointed at its own two `auth/` pages
(`scripts/auth-setup.ts`; see [auth.md](auth.md)). An authorization request
(the CLI's, an MCP client's) goes like this:

1. Neon Auth's `/oauth2/authorize` redirects to the login page when nobody is
   signed in. The page's query is the request itself, signed and valid for
   about 10 minutes.
2. **Sign-in** offers GitHub. GitHub returns to the same URL, query included.
   Whenever the page loads with a session, it resumes the request
   (`POST /oauth2/continue`), which goes on to consent. A 400 means the signed
   query has expired: the page says so, and the user starts again from the app.
3. **Consent** shows the client's name, the host the code is sent to, the
   scopes and the resources (the token's `aud`). A client's name is
   self-asserted, so when the code goes to a loopback address
   (`127.0.0.1`, `localhost`, `[::1]`) the page warns that only an app on this
   computer receives it. Allow or Deny (`POST /oauth2/consent`) returns to the
   client's redirect URI, with a code or `error=access_denied`.

The SDK (`@neon/neon-js`) sends the signed query back as `oauth_query` on
these calls and follows the redirect the server answers with, so the pages
never build those URLs themselves. Both `auth/` pages send
`Content-Security-Policy: frame-ancestors 'none'` ([`vercel.json`](../apps/web/vercel.json)),
so they can't be framed.

## Local development

```bash
bun install                                   # at the repo root; apps/web is a workspace
cp apps/web/.env.example apps/web/.env.local
# the endpoint id is the first label of the Base URL's host
bunx neon neon-auth status --branch staging --project-id cool-poetry-28343550
just web-dev
```

Then open `http://localhost:4322/b/<endpoint-id>/`. Neon Auth trusts
`localhost` by default, and GitHub's OAuth callback is Neon's
(`<Auth URL>/callback/github`). The branch needs the GitHub provider with
its own GitHub OAuth app (see the production checklist in
[auth.md](auth.md)); without it, sign-in fails with `PROVIDER_NOT_SUPPORTED`.

`just web-check` runs lint (Biome), typecheck and the unit tests, as the
[`web`](../.github/workflows/web.yml) workflow does on PRs; `just web-build`
writes `apps/web/dist`.

## Vercel

Vercel serves the build; [`vercel.json`](../apps/web/vercel.json) rewrites
every path to `index.html`, so deep links reach the router. The project is
created by hand, once, after `apps/web` is on `main` (Vercel only offers
directories that exist there):

1. **Add New → Project**, import `open-lakehouse/luminescent-lakehouse` with
   the GitHub (Git) integration.
2. **Root Directory**: `apps/web`. Framework preset: **Vite**.
3. Install command: `bun install` (Vercel finds the lockfile at the repo root,
   since `apps/web` is a Bun workspace). Build command `bun run build`, output
   directory `dist` (the preset's defaults).
4. Environment variables, for **Production, Preview and Development**:
   - `VITE_NEON_AUTH_HOST_SUFFIX` = `neonauth.c-7.eu-central-1.aws.neon.tech`
   - `VITE_NEON_AUTH_DB` = `neondb`
5. Production branch: `main`.

Don't add the Neon ↔ Vercel integration: it would create a Neon branch per
Vercel preview and inject one branch's Auth URL, and the app takes the branch
from the path instead. Vercel's preview deployments are for reviewing UI
changes only; any of them can be pointed at any branch through the path.

The production origin, `https://luminescent-lakehouse.vercel.app`, is a
trusted domain on every branch's Neon Auth (`authSetup` in `neon.ts`, applied
by `scripts/auth-setup.ts`), and every branch's OAuth server points at its own
`auth/` pages there ([auth.md](auth.md)). A Vercel preview URL is not trusted,
so it can show the pages but can't complete a sign-in.
