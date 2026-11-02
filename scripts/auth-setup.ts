// Reconcile a Neon branch's Neon Auth with `authSetup` in neon.ts. `neon.ts`
// can only switch Neon Auth on (`auth: true`), so this runs after every
// `neon deploy` (`just deploy`, neon-deploy.yml). It sets:
//
// - the OAuth server's protected resources: one per deployed function;
// - the trusted domains: adds missing ones, reports others;
// - once `oauthServer` is set, the OAuth server itself and the CLI's public
//   client.
//
// Neon Auth's settings live in the branch, so a new branch starts with its
// parent's; this brings an existing branch in line. Idempotent: a second run
// changes nothing. On the default (production) branch it also warns about the
// production checklist items it can't set, such as our own GitHub OAuth app.
//
// Usage: bun scripts/auth-setup.ts <neon-branch>
//
// Uses the logged-in CLI or NEON_API_KEY; NEON_PROJECT_ID selects the project
// (default: the linked one). Needs `bun install`.

import { execFileSync } from "node:child_process";
import config, { authSetup } from "../neon.ts";

interface OAuthServer {
  enabled: boolean;
  login_page?: string;
  consent_page?: string;
  mcp?: { resource?: string };
  resources: { identifier: string }[];
}

interface OAuthClient {
  client_id: string;
  name: string;
  type: string;
  redirect_uris: string[];
  resources: string[];
}

const branch = process.argv[2];
if (!branch) {
  console.error("usage: bun scripts/auth-setup.ts <neon-branch>");
  process.exit(2);
}
const project = process.env.NEON_PROJECT_ID ? ["--project-id", process.env.NEON_PROJECT_ID] : [];

// The CLI's errors go straight to stderr; a failed call fails the run.
function neon(args: string[]): string {
  return execFileSync("bunx", ["neon", ...args, ...project], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  });
}
function auth<T>(args: string[]): T {
  return JSON.parse(neon(["neon-auth", ...args, "--branch", branch, "-o", "json"])) as T;
}
function authChange(args: string[]): void {
  neon(["neon-auth", ...args, "--branch", branch]);
}

const sameSet = (a: string[], b: string[]): boolean =>
  a.length === b.length && [...a].sort().every((x, i) => x === [...b].sort()[i]);

const info = JSON.parse(neon(["branches", "get", branch, "-o", "json"])) as { default: boolean };
const setup = authSetup({ name: branch, isDefault: info.default });
let changed = 0;
const change = (what: string, args: string[]): void => {
  console.log(`→ ${what}`);
  authChange(args);
  changed++;
};

// Protected resources: exactly one per deployed function.
const resources = Object.keys(config.functions ?? {}).map((slug) => {
  const resource = setup.resources[slug];
  if (!resource) throw new Error(`authSetup has no OAuth resource for function '${slug}' (neon.ts)`);
  return resource;
});
let server = auth<OAuthServer>(["config", "oauth-server", "get"]);
if (!sameSet(resources, server.resources.map((r) => r.identifier))) {
  change(`OAuth resources: ${resources.join(", ")}`, ["config", "oauth-server", "update", "--resources", ...resources]);
}

// Trusted domains: added, never removed. Previews and local tools may add
// their own.
const domains = auth<{ domain: string }[]>(["domain", "list"]).map((d) => d.domain);
for (const domain of setup.trustedDomains.filter((d) => !domains.includes(d))) {
  change(`trusted domain ${domain}`, ["domain", "add", domain]);
}
for (const domain of domains.filter((d) => !setup.trustedDomains.includes(d))) {
  console.log(`  note: trusted domain ${domain} is not in neon.ts; left alone`);
}

// The OAuth server and the CLI client. The server can't be enabled without its
// login and consent pages, and clients can't exist without the server.
const want = setup.oauthServer;
if (want) {
  if (
    !server.enabled ||
    server.login_page !== want.loginPage ||
    server.consent_page !== want.consentPage ||
    (want.mcpResource !== undefined && server.mcp?.resource !== want.mcpResource)
  ) {
    const args = ["config", "oauth-server", "update", "--enabled", "--login-page", want.loginPage, "--consent-page", want.consentPage];
    if (want.mcpResource) args.push("--mcp-resource", want.mcpResource);
    change("OAuth server: enabled", args);
    server = auth<OAuthServer>(["config", "oauth-server", "get"]);
  }

  const cli = setup.cliClient;
  const existing = auth<OAuthClient[]>(["oauth-client", "list"]).filter((c) => c.name === cli.name);
  if (existing.length > 1) throw new Error(`more than one OAuth client is named '${cli.name}'; delete the extras`);
  const client = existing[0];
  if (!client) {
    change(`OAuth client '${cli.name}'`, [
      "oauth-client", "create", "--name", cli.name, "--type", "public",
      "--redirect-uris", ...cli.redirectUris, "--resources", ...resources,
    ]);
  } else if (client.type !== "public") {
    throw new Error(`OAuth client '${cli.name}' is ${client.type}, not public; delete it to recreate it`);
  } else if (!sameSet(client.redirect_uris, cli.redirectUris) || !sameSet(client.resources, resources)) {
    change(`OAuth client '${cli.name}': redirect URIs and resources`, [
      "oauth-client", "update", client.client_id,
      "--redirect-uris", ...cli.redirectUris, "--resources", ...resources,
    ]);
  }
} else if (server.enabled) {
  console.log("  note: the OAuth server is enabled, but neon.ts sets no oauthServer; left alone");
}

// Production checklist items that need credentials or a decision, so only
// reported (https://neon.com/docs/auth/production-checklist.md).
if (info.default) {
  const providers = auth<{ id: string; type: string }[]>(["oauth-provider", "list"]);
  const github = providers.find((p) => p.id === "github");
  if (!github || github.type === "shared") {
    console.log("  warning: production has no GitHub sign-in with our own OAuth app (neon neon-auth oauth-provider add --provider-id github …)");
  }
  for (const p of providers.filter((p) => p.type === "shared")) {
    console.log(`  warning: production uses Neon's shared ${p.id} OAuth app; it is meant for development`);
  }
}

console.log(changed ? `→ Neon Auth on ${branch}: ${changed} change(s)` : `→ Neon Auth on ${branch}: up to date`);
