// End-to-end OAuth sign-in on a preview branch, through the production web
// app's pages for that branch: authorization code + PKCE with `resource`, the
// sign-in and consent pages, the token exchange, a refresh, and a denied
// consent. It is the flow `headwaters-client login` runs, with an
// email/password user instead of GitHub (previews can't do GitHub sign-in).
// See docs/web.md ("End-to-end test").
//
// Run it with `just web-e2e preview/pr-<n>`; CI runs it after each preview
// deploy (.github/workflows/preview.yml).

import { randomBytes } from "node:crypto";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { expect, type Page, test } from "@playwright/test";
import {
  authorizeUrl,
  deleteUser,
  pkce,
  resolveTarget,
  type Target,
  type TokenResponse,
  token,
  verifyJwt,
} from "./neon";

/** Where the test records its user, for CI's cleanup backstop. */
const USER_FILE = "test-results/e2e-user.json";

let target: Target;
let userId: string | undefined;

test.beforeAll(() => {
  target = resolveTarget();
});

test.afterAll(() => {
  if (!userId) return;
  try {
    deleteUser(target.branch, userId);
    rmSync(USER_FILE, { force: true });
    console.log(`→ deleted test user ${userId}`);
  } catch (e) {
    console.error(`could not delete test user ${userId}: ${e}`);
  }
});

/** Answer the loopback redirect and resolve with its URL. */
function captureCallback(page: Page, t: Target): Promise<URL> {
  return new Promise((resolve) => {
    void page.route(`${t.redirectUri}**`, async (route) => {
      resolve(new URL(route.request().url()));
      await route.fulfill({ status: 200, contentType: "text/plain", body: "signed in" });
    });
  });
}

test("authorization code + PKCE through the sign-in and consent pages", async ({ page }) => {
  const t = target;
  const run = process.env.GITHUB_RUN_ID ?? "local";
  const email = `e2e+${run}-${randomBytes(4).toString("hex")}@example.com`;
  const password = randomBytes(18).toString("base64url");
  const allow = pkce();
  let tokens: TokenResponse | undefined;

  await test.step("authorize sends the browser to the sign-in page", async () => {
    await page.goto(authorizeUrl(t, allow));
    await expect(page).toHaveURL((u) => u.href.startsWith(`${t.pages}/sign-in?`));
    expect(new URL(page.url()).searchParams.has("sig")).toBe(true);
    await expect(page.getByText("Sign in", { exact: true })).toBeVisible();
  });

  const consentUrl = await test.step("sign up from the sign-in page", async () => {
    // From inside the page, so the request has the web app's Origin and the
    // session cookie lands in the browser, as the SDK's own calls do.
    const signUp = await page.evaluate(
      async ({ authUrl, email, password }) => {
        const res = await fetch(`${authUrl}/sign-up/email`, {
          method: "POST",
          credentials: "include",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            email,
            password,
            name: "e2e",
            oauth_query: window.location.search.slice(1),
          }),
        });
        return { status: res.status, body: await res.json() };
      },
      { authUrl: t.authUrl, email, password },
    );
    expect(signUp.status, JSON.stringify(signUp.body)).toBe(200);

    // The session cookie is third-party here (vercel.app page, neon.tech
    // Auth): this proves the browser kept it and sends it back.
    const session = await page.evaluate(async (authUrl) => {
      const res = await fetch(`${authUrl}/get-session`, { credentials: "include" });
      return (await res.json()) as { user?: { id: string; email: string } } | null;
    }, t.authUrl);
    expect(session?.user?.email).toBe(email);
    userId = session?.user?.id;
    mkdirSync("test-results", { recursive: true });
    writeFileSync(USER_FILE, JSON.stringify({ id: userId, email }));

    const body = signUp.body as { redirect?: boolean; url?: string };
    expect(body.redirect).toBe(true);
    expect(body.url?.startsWith(`${t.pages}/consent?`), body.url).toBe(true);
    return body.url as string;
  });

  await test.step("allow on the consent page", async () => {
    const callback = captureCallback(page, t);
    await page.goto(consentUrl);
    await expect(page.getByText(`Signed in as ${email}`)).toBeVisible();
    await expect(page.getByText(t.resource)).toBeVisible();
    await expect(page.getByRole("note")).toContainText("Only an app running on this computer");
    await page.getByRole("button", { name: "Allow" }).click();

    const url = await callback;
    expect(url.searchParams.get("state")).toBe(allow.state);
    expect(url.searchParams.get("error")).toBeNull();
    expect(url.searchParams.get("iss")).toBe(t.authUrl);
    const code = url.searchParams.get("code");
    expect(code).toBeTruthy();

    const res = await token(t, {
      grant_type: "authorization_code",
      code: code as string,
      redirect_uri: t.redirectUri,
      code_verifier: allow.verifier,
    });
    expect(res.status, JSON.stringify(res.body)).toBe(200);
    tokens = res.body as unknown as TokenResponse;
  });

  await test.step("the access token is a JWT for the headwaters resource", async () => {
    const tok = tokens as TokenResponse;
    expect(tok.token_type.toLowerCase()).toBe("bearer");
    expect(tok.expires_in).toBeGreaterThan(0);
    expect(tok.refresh_token).toBeTruthy();
    expect(tok.id_token).toBeTruthy();

    const { header, payload } = await verifyJwt(t, tok.access_token);
    expect(header.typ).toBe("at+jwt");
    expect(header.alg).toBe("EdDSA");
    expect(payload.iss).toBe(t.authUrl);
    expect([payload.aud].flat()).toContain(t.resource);
    expect(payload.sub).toBe(userId);
    expect(payload.exp ?? 0).toBeGreaterThan(Date.now() / 1000);
  });

  await test.step("a refresh rotates the refresh token", async () => {
    const old = (tokens as TokenResponse).refresh_token as string;
    const res = await token(t, { grant_type: "refresh_token", refresh_token: old });
    expect(res.status, JSON.stringify(res.body)).toBe(200);
    const fresh = res.body as unknown as TokenResponse;
    expect(fresh.refresh_token).toBeTruthy();
    expect(fresh.refresh_token).not.toBe(old);
    const { payload } = await verifyJwt(t, fresh.access_token);
    expect([payload.aud].flat()).toContain(t.resource);

    const reused = await token(t, { grant_type: "refresh_token", refresh_token: old });
    expect(reused.status).toBe(400);
    expect(reused.body.error).toBe("invalid_grant");
  });

  await test.step("deny on the consent page returns access_denied", async () => {
    const deny = pkce();
    const callback = captureCallback(page, t);
    // Already signed in and consented: `prompt=consent` asks again.
    await page.goto(authorizeUrl(t, deny, { prompt: "consent" }));
    await expect(page).toHaveURL((u) => u.href.startsWith(`${t.pages}/consent?`));
    await page.getByRole("button", { name: "Deny" }).click();

    const url = await callback;
    expect(url.searchParams.get("error")).toBe("access_denied");
    expect(url.searchParams.get("state")).toBe(deny.state);
    expect(url.searchParams.get("code")).toBeNull();
  });
});
