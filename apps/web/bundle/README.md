# luminescent-web

The Neon Auth pages of a luminescent lakehouse: the OAuth server's login and
consent pages, plus a page that shows who is signed in. It is a static
single-page app with no configuration, so one copy serves every branch of
every Neon project. Each page is at `/a/` plus the branch's Auth URL without
the scheme:

```
https://<your host>/a/<endpoint-id>.neonauth.<region…>.neon.tech/<db>/auth/sign-in
https://<your host>/a/<endpoint-id>.neonauth.<region…>.neon.tech/<db>/auth/consent
```

The pages store nothing. Users, sessions and OAuth clients stay in each
branch's Neon Auth.

## Hosting

Serve this directory as a static site on any host that sends every path to
`index.html`:

- **Vercel**: `vercel deploy --prod` from this directory (`vercel.json`).
- **Netlify** or **Cloudflare Pages**: upload this directory (`_redirects`,
  `_headers`).
- **Anywhere else**: rewrite every path to `/index.html`, and send
  `Content-Security-Policy: frame-ancestors 'none'` on `/a/*/*/auth/*`.

Then add this site's origin to each branch's Neon Auth trusted domains, and
point the branch's OAuth server's login and consent pages at it.
