# Web app

[`apps/web`](../apps/web) holds the browser pages for Neon Auth: the OAuth
server's login and consent pages, and a page that signs in with GitHub and
shows who you are. It is a static Vite + React SPA (shadcn components, Tailwind 4, the docs-factory
theme), tracked in
[#33](https://github.com/open-lakehouse/luminescent-lakehouse/issues/33).

## One deployment for every branch

Every Neon branch has its own Neon Auth server (see [auth.md](auth.md)), so
its pages have to talk to that branch's Auth URL. Instead of one deployment
per branch, the path names the Auth server: `/a/` plus the branch's Auth URL
without the scheme.

```
Auth URL:  https://<endpoint-id>.neonauth.<region…>.neon.tech/<db>/auth
pages:     https://<app host>/a/<endpoint-id>.neonauth.<region…>.neon.tech/<db>/…
```

A new branch works as soon as it exists, and so does a branch of any other
Neon project in any region, without a redeploy and without any configuration
in the build. The app only accepts `ep-….neonauth.….neon.tech` hosts, so its
pages can't be pointed at a server that isn't Neon's. Anything else renders
the 404 page.

The older form, `/b/<endpoint-id>/…`, rebuilt the Auth URL from two build-time
settings our branches share
(`https://<endpoint-id>.<VITE_NEON_AUTH_HOST_SUFFIX>/<VITE_NEON_AUTH_DB>/auth`).
It still works where those are set (our Vercel deployment), until every
branch's OAuth server has been repointed at `/a/` (each deploy does that).
Builds without them, such as the self-hosting tarball, serve `/a/` only.

## Pages

| Path, under `/a/<auth-host>/<db>` | What |
|---|---|
| `/` | Who's signed in on that branch; GitHub sign-in and sign-out. For checking a branch by hand. |
| `/auth/sign-in` | The OAuth server's login page. |
| `/auth/consent` | The OAuth server's consent page. |

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
# the branch's Auth URL is the Base URL
bunx neon neon-auth status --branch staging --project-id cool-poetry-28343550
just web-dev
```

Then open `http://localhost:4322/a/<auth-host>/<db>/`, the Base URL without
`https://` and `/auth`. The older `/b/<endpoint-id>/` pages also need
`apps/web/.env.local` (`cp apps/web/.env.example apps/web/.env.local`). Neon Auth trusts
`localhost` by default, and GitHub's OAuth callback is Neon's
(`<Auth URL>/callback/github`). The branch needs the GitHub provider with
its own GitHub OAuth app (see the production checklist in
[auth.md](auth.md)); without it, sign-in fails with `PROVIDER_NOT_SUPPORTED`.

`just web-check` runs lint (Biome), typecheck and the unit tests, as the
[`web`](../.github/workflows/web.yml) workflow does on PRs; `just web-build`
writes `apps/web/dist`, and `just web-bundle` the self-hosting tarball.

## End-to-end test

[`apps/web/e2e/`](../apps/web/e2e/) holds a Playwright test of the whole OAuth
sign-in, through the production web app's pages for one preview branch. It runs
the flow `headwaters-client login` uses: the `luminescent CLI` public client,
authorization code + PKCE (S256) with `resource` set to the headwaters
resource, and the loopback redirect `http://127.0.0.1:8765/callback`, which the
test intercepts in the browser.

1. The browser opens `<Auth URL>/oauth2/authorize`, which redirects to the
   sign-in page.
2. The page only offers GitHub, and previews can't do GitHub sign-in: they
   inherit staging's GitHub app, whose callback is staging's. So the test signs
   up a throwaway email/password user (`e2e+<run>-<random>@example.com`) with a
   `fetch` from inside the sign-in page, passing the page's signed query as
   `oauth_query`. The request carries the web app's `Origin`, and the session
   cookie lands in the browser. Neon Auth answers with the consent URL.
3. The consent page shows the signed-in user and the resource. The test
   clicks **Allow**, then exchanges the code at `/oauth2/token`.
4. It checks the access token: `typ: at+jwt`, its EdDSA signature (against the
   branch's JWKS), `iss` = the Auth URL, and an `aud` containing the resource.
   A refresh returns a new refresh token, and the old one then gets
   `invalid_grant`.
5. It authorizes again with `prompt=consent`, clicks **Deny**, and expects
   `error=access_denied`.
6. It deletes the user (`neon neon-auth user delete`).

The session cookie is third-party on the pages (the page is on vercel.app, the
Auth server on neon.tech), as it is for real users. Playwright's Chromium keeps
it and sends it back with its default settings. The test checks this with
`get-session` right after sign-up.

The [`preview`](../.github/workflows/preview.yml) workflow runs the test in its
`e2e` job after each deploy. If the test dies before deleting its user, that
job deletes it (the id is in `test-results/e2e-user.json`). The test resolves
the Auth URL and the client id with the Neon CLI and takes the rest from
`authSetup` in `neon.ts`. It refuses any branch that isn't `preview/*`, so it
can't reach staging or production. It always tests the pages on `main`'s
Vercel deployment, never a PR's own changes to `apps/web`.

By hand, against an open PR's preview (needs `neon login`):

```bash
(cd apps/web && bunx playwright install chromium)   # once
just web-e2e preview/pr-<n>
```

`bun test` (`just web-check`) never runs it: its files are `*.e2e.ts`.

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
4. Environment variables, for **Production, Preview and Development**, only
   for the older `/b/` pages (drop them once no branch points there):
   - `VITE_NEON_AUTH_HOST_SUFFIX` = `neonauth.c-7.eu-central-1.aws.neon.tech`
   - `VITE_NEON_AUTH_DB` = `neondb`
5. Production branch: `main`.

Don't add the Neon ↔ Vercel integration: it would create a Neon branch per
Vercel preview and inject one branch's Auth URL, and the app takes the branch
from the path instead. Vercel's preview deployments are for reviewing UI
changes only; any of them can be pointed at any branch through the path.

The production origin, `https://luminescent-lakehouse.vercel.app`
(`TENANT.webOrigin` in `neon.ts`), is a trusted domain on every branch's Neon
Auth (`authSetup`, applied by `scripts/auth-setup.ts`), and every branch's OAuth server points at its own
`auth/` pages there ([auth.md](auth.md)). A Vercel preview URL is not trusted,
so it can show the pages but can't complete a sign-in.

## Self-hosting

A lakehouse deployed from another repository can use this deployment or serve
the pages itself. Neither choice moves its users: they, their sessions, the
GitHub OAuth app and the OAuth clients all stay in each branch's Neon Auth, in
the deployer's Neon project. The pages store nothing. What a deployer trusts
when they use the shared deployment:

- its JavaScript, which runs with their users' sessions, because their
  branches list its origin as a trusted domain;
- its host's request logs, which see the Auth hosts in the paths.

To serve the pages yourself, take `luminescent-web-<version>.tar.gz` from a
release (or `just web-bundle`), host its directory as a static site, and set
`webOrigin` in `TENANT` in your `neon.ts` to the site's origin. The tarball
carries `vercel.json`, plus `_headers` and `_redirects` for Netlify and
Cloudflare Pages, and a README covering other hosts. The next deploy points
each branch's OAuth server at your pages and adds your origin to its trusted
domains. It doesn't remove the old origin; remove that by hand
(`neon neon-auth domain`) if you no longer trust it.
