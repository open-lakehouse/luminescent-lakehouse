// Registers a branch's storage with its Unity Catalog, after each deploy to a
// branch with credential vending on (`vending` in neon.ts): the `neon`
// credential for the branch's own storage, and the `lakehouse` external
// location over the bucket neon.ts declares. The managed storage root is
// config (neon.ts), not an API object. Running it again changes nothing.
//
// Usage: bun scripts/storage-seed.ts <neon-branch>
//
// It goes through UC's own API, as a dedicated service account,
// `uc-storage-seed`, with `create` and `manage` on the metastore. Its API key
// is issued for this run and revoked at the end, so no secret is kept for it.
// The account and its grants are created on the first run (and inherited by
// child branches); `luminescent-admin` reaches `authz` over the branch's direct
// connection string. NEON_PROJECT_ID selects the project (default: the linked
// one); LUMINESCENT_ADMIN is a prebuilt `luminescent-admin` (CI), else `cargo run`.
//
// A child branch inherits its parent's registrations, and with them the
// parent's branch id in the credential's ARN. Keys minted on the parent work
// on the parent too, so the credential is always repointed at this branch:
// a preview never vends staging's keys.

import { execFileSync } from "node:child_process";
import { LAKEHOUSE_BUCKET, STORAGE_CREDENTIAL, TENANT, vending } from "../neon.ts";

export const SEED_ACCOUNT = "uc-storage-seed";
export const EXTERNAL_LOCATION = LAKEHOUSE_BUCKET;
const API = "/api/2.1/unity-catalog";

/** The ARN of a branch's Neon storage, as the catalog stores it. */
export function neonArn(projectId: string, branchId: string): string {
  return `arn:neon:storage::${projectId}:branch/${branchId}`;
}

export interface Step {
  method: "GET" | "POST" | "PATCH";
  path: string;
  body?: unknown;
}

/** What makes the credential right: create it, or repoint an existing one at this branch. */
export function credentialStep(exists: boolean, arn: string): Step {
  const aws_iam_role = { role_arn: arn };
  return exists
    ? { method: "PATCH", path: `${API}/credentials/${STORAGE_CREDENTIAL}`, body: { aws_iam_role } }
    : {
        method: "POST",
        path: `${API}/credentials`,
        body: { name: STORAGE_CREDENTIAL, purpose: "STORAGE", aws_iam_role, comment: "the branch's Neon storage" },
      };
}

/** What makes the external location right: create it, or point it back at the bucket and the credential. */
export function locationStep(existing: { url?: string; credential_name?: string } | undefined): Step | undefined {
  const url = `s3://${LAKEHOUSE_BUCKET}`;
  if (!existing) {
    return {
      method: "POST",
      path: `${API}/external-locations`,
      body: { name: EXTERNAL_LOCATION, url, credential_name: STORAGE_CREDENTIAL, comment: "the branch's lakehouse bucket" },
    };
  }
  if (existing.url?.replace(/\/+$/, "") === url && existing.credential_name === STORAGE_CREDENTIAL) return undefined;
  return {
    method: "PATCH",
    path: `${API}/external-locations/${EXTERNAL_LOCATION}`,
    body: { url, credential_name: STORAGE_CREDENTIAL },
  };
}

type Call = (step: Step) => Promise<{ status: number; json: unknown; text: string }>;

/** Register the branch's storage through `call`, a client of its catalog. */
export async function seed(call: Call, arn: string): Promise<string[]> {
  const done: string[] = [];
  const must = async (step: Step) => {
    const res = await call(step);
    if (res.status !== 200) throw new Error(`${step.method} ${step.path}: ${res.status} ${res.text}`);
    done.push(`${step.method} ${step.path}`);
    return res;
  };

  const credential = await call({ method: "GET", path: `${API}/credentials/${STORAGE_CREDENTIAL}` });
  if (credential.status !== 200 && credential.status !== 404) {
    throw new Error(`GET credential ${STORAGE_CREDENTIAL}: ${credential.status} ${credential.text}`);
  }
  await must(credentialStep(credential.status === 200, arn));

  const location = await call({ method: "GET", path: `${API}/external-locations/${EXTERNAL_LOCATION}` });
  if (location.status !== 200 && location.status !== 404) {
    throw new Error(`GET external location ${EXTERNAL_LOCATION}: ${location.status} ${location.text}`);
  }
  const step = locationStep(location.status === 200 ? (location.json as { url?: string; credential_name?: string }) : undefined);
  if (step) await must(step);
  return done;
}

// --- the deploy step ---------------------------------------------------------

function neon(args: string[]): string {
  const project = process.env.NEON_PROJECT_ID ? ["--project-id", process.env.NEON_PROJECT_ID] : [];
  return execFileSync("bunx", ["neon", ...args, ...project, "-o", "json"], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "inherit"],
  });
}

/** The JSON document in the CLI's output, which may carry notices around it. */
function json<T>(out: string): T {
  const start = out.search(/[[{]/);
  return JSON.parse(out.slice(start, Math.max(out.lastIndexOf("}"), out.lastIndexOf("]")) + 1)) as T;
}

function admin(databaseUrl: string, args: string[]): string {
  const env = { ...process.env, DATABASE_URL: databaseUrl, SQLX_OFFLINE: "true" };
  const [cmd, pre] = process.env.LUMINESCENT_ADMIN
    ? [process.env.LUMINESCENT_ADMIN, []]
    : ["cargo", ["run", "-q", "-p", "luminescent-migrate", "--bin", "luminescent-admin", "--"]];
  return execFileSync(cmd, [...pre, ...args], { encoding: "utf8", env, stdio: ["ignore", "pipe", "inherit"] }).trim();
}

async function main(branch: string) {
  if (!vending(branch).enabled) {
    console.log(`→ vending is off on ${branch}; nothing to register`);
    return;
  }
  const projectId = process.env.NEON_PROJECT_ID ?? TENANT.neonProjectId;
  const info = json<{ id?: string; branch?: { id: string } }>(neon(["branches", "get", branch]));
  const branchId = info.branch?.id ?? info.id;
  if (!branchId) throw new Error(`no id for Neon branch ${branch}`);
  const fn = json<{ invocation_url?: string; function?: { invocation_url: string } }>(
    neon(["functions", "get", "unitycatalog", "--branch", branch]),
  );
  const base = (fn.invocation_url ?? fn.function?.invocation_url ?? "").replace(/\/+$/, "");
  if (!base) throw new Error(`no unitycatalog function on ${branch}`);

  const project = process.env.NEON_PROJECT_ID ? ["--project-id", process.env.NEON_PROJECT_ID] : [];
  const databaseUrl = execFileSync(
    "bunx",
    ["neon", "connection-string", branch, "--database-name", "neondb", ...project],
    { encoding: "utf8", stdio: ["ignore", "pipe", "inherit"] },
  ).trim();
  if (process.env.GITHUB_ACTIONS) console.log(`::add-mask::${databaseUrl}`);

  admin(databaseUrl, ["service-account", "create", SEED_ACCOUNT, "--if-missing", "--description", "registers the branch's storage with Unity Catalog (scripts/storage-seed.ts)"]);
  for (const relation of ["create", "manage"]) {
    admin(databaseUrl, ["grant", "add", `service-account:${SEED_ACCOUNT}`, relation, "UnityCatalog::Metastore", "metastore"]);
  }
  const key = admin(databaseUrl, ["api-key", "issue", SEED_ACCOUNT, "--expires-in-days", "1", "--description", `storage-seed ${new Date().toISOString()}`]);
  if (process.env.GITHUB_ACTIONS) console.log(`::add-mask::${key}`);
  const keyId = /^olk_([0-9a-f]{16})_/.exec(key)?.[1];
  if (!keyId) throw new Error("luminescent-admin printed no API key");

  try {
    const call: Call = async (step) => {
      // The function may still be starting after the deploy (a cold isolate).
      for (let attempt = 0; ; attempt++) {
        const res = await fetch(base + step.path, {
          method: step.method,
          headers: { authorization: `Bearer ${key}`, ...(step.body ? { "content-type": "application/json" } : {}) },
          body: step.body ? JSON.stringify(step.body) : undefined,
        });
        const text = await res.text();
        if (res.status === 503 && attempt < 5) {
          await new Promise((r) => setTimeout(r, 5000));
          continue;
        }
        let parsed: unknown;
        try {
          parsed = JSON.parse(text);
        } catch {}
        return { status: res.status, json: parsed, text };
      }
    };
    const done = await seed(call, neonArn(projectId, branchId));
    for (const d of done) console.log(`→ ${d}`);
    console.log(`→ ${branch}: credential ${STORAGE_CREDENTIAL} → ${neonArn(projectId, branchId)}, location ${EXTERNAL_LOCATION} → s3://${LAKEHOUSE_BUCKET}`);
  } finally {
    admin(databaseUrl, ["api-key", "revoke", keyId]);
  }
}

if (import.meta.main) {
  const branch = process.argv[2];
  if (!branch) {
    console.error("usage: bun scripts/storage-seed.ts <neon-branch>");
    process.exit(2);
  }
  await main(branch);
}
