-- The callers smoke.test.mjs authenticates as, in the test database (neondb's
-- stand-in): two Neon Auth users and a service account with an API key. A
-- plain Postgres has no `neon_auth`, so its `user` table is created here with
-- the columns the server reads (the layout of crates/migrate/tests/authz.rs).
-- `authz` comes from `luminescent-migrate authz`. Safe to run again.

CREATE SCHEMA IF NOT EXISTS neon_auth;
CREATE TABLE IF NOT EXISTS neon_auth."user" (
    id            uuid PRIMARY KEY,
    email         text,
    "emailVerified" bool,
    role          text,
    banned        bool,
    "banReason"   text,
    "banExpires"  timestamptz
);

INSERT INTO neon_auth."user" (id, email, "emailVerified", role, banned)
VALUES
    ('5e1f0000-0000-4000-8000-000000000001', 'smoke@example.com', true, NULL, false),
    ('5e1f0000-0000-4000-8000-000000000002', 'banned@example.com', true, NULL, true)
ON CONFLICT (id) DO UPDATE SET role = EXCLUDED.role, banned = EXCLUDED.banned, "banExpires" = NULL;

INSERT INTO authz.service_accounts (id, name, description)
VALUES ('5e1f0000-0000-4000-8000-0000000000a1', 'uc-smoke', 'unitycatalog-neon smoke tests')
ON CONFLICT (id) DO UPDATE SET disabled_at = NULL;

-- The key smoke.test.mjs sends: olk_5e1f00000000a001_<64 hex>.
INSERT INTO authz.api_keys (id, account_id, sha256)
VALUES (
    '5e1f00000000a001',
    '5e1f0000-0000-4000-8000-0000000000a1',
    sha256(convert_to('olk_5e1f00000000a001_' || repeat('5e', 32), 'UTF8'))
)
ON CONFLICT (id) DO UPDATE SET sha256 = EXCLUDED.sha256, revoked_at = NULL, expires_at = NULL;
