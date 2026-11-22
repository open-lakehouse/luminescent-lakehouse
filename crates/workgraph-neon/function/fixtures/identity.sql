-- The callers the function's tests authenticate as, in the test database
-- (neondb's stand-in): three Neon Auth users, and four service accounts with
-- API keys, plus a revoked and an expired key. One account has no entry of its
-- own and gets its grant through a group, two levels up. A plain Postgres has no
-- `neon_auth`, so its `user` table is created here with the columns the server
-- reads. `authz` comes from `luminescent-migrate authz`. The ids are this
-- fixture's own (3c0f…), apart from unitycatalog's and authz's. Safe to run
-- again.

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
    -- The operator: an apps principal.
    ('3c0f0000-0000-4000-8000-000000000001', 'operator@example.com', true, NULL, false),
    -- A user with an entry, but no apps.
    ('3c0f0000-0000-4000-8000-000000000002', 'watcher@example.com', true, NULL, false),
    ('3c0f0000-0000-4000-8000-000000000003', 'banned@example.com', true, NULL, true)
ON CONFLICT (id) DO UPDATE SET role = EXCLUDED.role, banned = EXCLUDED.banned, "banExpires" = NULL;

INSERT INTO authz.service_accounts (id, name, description)
VALUES
    ('3c0f0000-0000-4000-8000-0000000000a1', 'wg-planner', 'workgraph-neon smoke tests: the planner'),
    ('3c0f0000-0000-4000-8000-0000000000a2', 'wg-reviewer', 'workgraph-neon smoke tests: the reviewer'),
    ('3c0f0000-0000-4000-8000-0000000000a3', 'wg-stranger', 'workgraph-neon smoke tests: no grant'),
    ('3c0f0000-0000-4000-8000-0000000000a6', 'wg-member', 'workgraph-neon smoke tests: a planner by group')
ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, disabled_at = NULL;

-- wg-member is in wg-env, which is in wg-planners (FACTORY_MCP_PRINCIPALS maps
-- the latter).
INSERT INTO authz.groups (id, name, description)
VALUES
    ('3c0f0000-0000-4000-8000-0000000000b1', 'wg-planners', 'workgraph-neon smoke tests: planners'),
    ('3c0f0000-0000-4000-8000-0000000000b2', 'wg-env', 'workgraph-neon smoke tests: one environment''s agents')
ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name;
INSERT INTO authz.memberships (group_id, principal_type, principal_id)
VALUES
    ('3c0f0000-0000-4000-8000-0000000000b1', 'group', '3c0f0000-0000-4000-8000-0000000000b2'),
    ('3c0f0000-0000-4000-8000-0000000000b2', 'service_account', '3c0f0000-0000-4000-8000-0000000000a6')
ON CONFLICT DO NOTHING;

-- The keys smoke.test.mjs sends: olk_<id>_<64 hex>, the hex `3c` repeated.
INSERT INTO authz.api_keys (id, account_id, sha256, revoked_at, expires_at)
SELECT k.id, k.account_id::uuid,
       sha256(convert_to('olk_' || k.id || '_' || repeat('3c', 32), 'UTF8')),
       k.revoked_at, k.expires_at
FROM (VALUES
    ('3c0f00000000a001', '3c0f0000-0000-4000-8000-0000000000a1', NULL::timestamptz, NULL::timestamptz),
    ('3c0f00000000a002', '3c0f0000-0000-4000-8000-0000000000a2', NULL, NULL),
    ('3c0f00000000a003', '3c0f0000-0000-4000-8000-0000000000a3', NULL, NULL),
    -- The planner's, revoked and expired.
    ('3c0f00000000a004', '3c0f0000-0000-4000-8000-0000000000a1', now() - interval '1 hour', NULL),
    ('3c0f00000000a005', '3c0f0000-0000-4000-8000-0000000000a1', NULL, now() - interval '1 hour'),
    ('3c0f00000000a006', '3c0f0000-0000-4000-8000-0000000000a6', NULL, NULL)
) AS k (id, account_id, revoked_at, expires_at)
ON CONFLICT (id) DO UPDATE
SET sha256 = EXCLUDED.sha256, revoked_at = EXCLUDED.revoked_at, expires_at = EXCLUDED.expires_at;
