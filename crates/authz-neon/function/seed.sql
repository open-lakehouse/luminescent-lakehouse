-- The fixture principals, groups and grants of the authz tests
-- (policies/tests/*.json, *.test.mjs), on the local test Postgres after
-- `luminescent-migrate authz`. `just test authz` and `just test-arm64 authz`
-- apply it; rerunning resets the fixture rows and leaves the rest alone.
--
-- A plain Postgres has no Neon Auth, so this creates a stand-in
-- `neon_auth."user"` with the columns the PDP reads (Neon Auth's layout, as in
-- crates/migrate/tests/authz.rs).

BEGIN;

DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'neon_auth') THEN
        CREATE ROLE neon_auth NOLOGIN;
    END IF;
END
$$;
CREATE SCHEMA IF NOT EXISTS neon_auth AUTHORIZATION neon_auth;
CREATE TABLE IF NOT EXISTS neon_auth."user" (
    id              uuid PRIMARY KEY,
    email           text,
    "emailVerified" bool,
    role            text,
    banned          bool,
    "banReason"     text,
    "banExpires"    timestamptz
);

-- Users: aaaaaaaa-…; service accounts: bbbbbbbb-…; groups: cccccccc-….
DELETE FROM authz.grants
WHERE subject_id LIKE 'aaaaaaaa-%' OR subject_id LIKE 'bbbbbbbb-%' OR subject_id LIKE 'cccccccc-%';
DELETE FROM authz.memberships WHERE group_id::text LIKE 'cccccccc-%';
DELETE FROM authz.groups WHERE id::text LIKE 'cccccccc-%';
DELETE FROM authz.service_accounts WHERE id::text LIKE 'bbbbbbbb-%';
DELETE FROM neon_auth."user" WHERE id::text LIKE 'aaaaaaaa-%';

INSERT INTO neon_auth."user" (id, email, "emailVerified", role, banned, "banExpires") VALUES
    -- An admin ("Make admin" in the Console).
    ('aaaaaaaa-0000-0000-0000-000000000001', 'admin@example.com', true, 'admin', false, NULL),
    -- A user without a grant.
    ('aaaaaaaa-0000-0000-0000-000000000002', 'plain@example.com', true, 'user', false, NULL),
    -- In `analysts`, which is in `data`, which holds `select` and `read` on main.sales.
    ('aaaaaaaa-0000-0000-0000-000000000003', 'analyst@example.com', true, 'user', false, NULL),
    -- Holds `select` and `external_use` on main.sales itself.
    ('aaaaaaaa-0000-0000-0000-000000000004', 'vendor@example.com', true, NULL, false, NULL),
    -- A banned admin.
    ('aaaaaaaa-0000-0000-0000-000000000005', 'banned@example.com', true, 'admin', true, NULL),
    -- A ban that has expired; holds `select` on the catalog `main`.
    ('aaaaaaaa-0000-0000-0000-000000000006', 'pardoned@example.com', false, 'user', true, now() - interval '1 day'),
    -- Made admin and back by smoke.test.mjs.
    ('aaaaaaaa-0000-0000-0000-000000000007', 'flip@example.com', true, 'user', false, NULL),
    -- Unity Catalog, one relation each (policies/tests/unitycatalog.json):
    -- `browse` on the metastore.
    ('aaaaaaaa-0000-0000-0000-000000000008', 'browser@example.com', true, 'user', false, NULL),
    -- `modify` on the catalog `main`.
    ('aaaaaaaa-0000-0000-0000-00000000000e', 'writer@example.com', true, 'user', false, NULL),
    -- `create` on the schema `main.sales`.
    ('aaaaaaaa-0000-0000-0000-00000000000a', 'creator@example.com', true, 'user', false, NULL),
    -- `manage` on the table `main.sales.orders`.
    ('aaaaaaaa-0000-0000-0000-00000000000b', 'manager@example.com', true, 'user', false, NULL),
    -- `all_privileges` on the catalog `main`.
    ('aaaaaaaa-0000-0000-0000-00000000000c', 'owner@example.com', true, 'user', false, NULL),
    -- `use` on the credential `landing-cred`.
    ('aaaaaaaa-0000-0000-0000-00000000000d', 'user@example.com', true, 'user', false, NULL);

INSERT INTO authz.service_accounts (id, name, disabled_at) VALUES
    ('bbbbbbbb-0000-0000-0000-000000000001', 'fixture-ingest', NULL),
    ('bbbbbbbb-0000-0000-0000-000000000002', 'fixture-disabled', now());

INSERT INTO authz.groups (id, name) VALUES
    ('cccccccc-0000-0000-0000-000000000001', 'fixture-analysts'),
    ('cccccccc-0000-0000-0000-000000000002', 'fixture-data');

INSERT INTO authz.memberships (group_id, principal_type, principal_id) VALUES
    ('cccccccc-0000-0000-0000-000000000001', 'user', 'aaaaaaaa-0000-0000-0000-000000000003'),
    ('cccccccc-0000-0000-0000-000000000002', 'group', 'cccccccc-0000-0000-0000-000000000001');

INSERT INTO authz.grants (subject_type, subject_id, relation, object_type, object_id) VALUES
    ('group', 'cccccccc-0000-0000-0000-000000000002', 'select', 'UnityCatalog::Schema', 'main.sales'),
    ('group', 'cccccccc-0000-0000-0000-000000000002', 'read', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000004', 'select', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000004', 'external_use', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000006', 'select', 'UnityCatalog::Catalog', 'main'),
    ('service_account', 'bbbbbbbb-0000-0000-0000-000000000001', 'select', 'UnityCatalog::Catalog', 'main'),
    ('service_account', 'bbbbbbbb-0000-0000-0000-000000000002', 'select', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000008', 'browse', 'UnityCatalog::Metastore', 'metastore'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000e', 'modify', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000a', 'create', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000b', 'manage', 'UnityCatalog::Table', 'main.sales.orders'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000c', 'all_privileges', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000d', 'use', 'UnityCatalog::Credential', 'landing-cred'),
    -- Path credentials: EXTERNAL USE (and read) on the external location itself.
    ('service_account', 'bbbbbbbb-0000-0000-0000-000000000001', 'read', 'UnityCatalog::ExternalLocation', 'landing'),
    ('service_account', 'bbbbbbbb-0000-0000-0000-000000000001', 'external_use', 'UnityCatalog::ExternalLocation', 'landing');

COMMIT;
