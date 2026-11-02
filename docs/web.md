# Web app

[`apps/web`](../apps/web) holds the browser pages for Neon Auth: today a page
that signs in with GitHub and shows who you are; the sign-in and OAuth consent
pages come next
([#35](https://github.com/open-lakehouse/luminescent-lakehouse/issues/35)). It
is a static Vite + React SPA (shadcn components, Tailwind 4, the docs-factory
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
(`<Auth URL>/callback/github`), so no other setup is needed.

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

Before a deployed page can sign anyone in, its domain must be a trusted domain
on each branch's Neon Auth. That lands with the sign-in pages in
[#35](https://github.com/open-lakehouse/luminescent-lakehouse/issues/35).
