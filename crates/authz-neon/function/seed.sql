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
    -- In `analysts`, which is in `data`: USE_CATALOG on main, USE_SCHEMA and
    -- SELECT on main.sales.
    ('aaaaaaaa-0000-0000-0000-000000000003', 'analyst@example.com', true, 'user', false, NULL),
    -- Usage of main.sales, SELECT on main.sales.orders, EXTERNAL_USE_SCHEMA on main.sales.
    ('aaaaaaaa-0000-0000-0000-000000000004', 'vendor@example.com', true, NULL, false, NULL),
    -- A banned admin.
    ('aaaaaaaa-0000-0000-0000-000000000005', 'banned@example.com', true, 'admin', true, NULL),
    -- A ban that has expired; USE_CATALOG, USE_SCHEMA and SELECT on the catalog main.
    ('aaaaaaaa-0000-0000-0000-000000000006', 'pardoned@example.com', false, 'user', true, now() - interval '1 day'),
    -- Made admin and back by smoke.test.mjs; owns main.sales.orders.
    ('aaaaaaaa-0000-0000-0000-000000000007', 'flip@example.com', true, 'user', false, NULL),
    -- BROWSE on the metastore.
    ('aaaaaaaa-0000-0000-0000-000000000008', 'browser@example.com', true, 'user', false, NULL),
    -- Usage of main, CREATE_SCHEMA on it.
    ('aaaaaaaa-0000-0000-0000-000000000009', 'architect@example.com', true, 'user', false, NULL),
    -- Usage of main.sales, CREATE_TABLE and EXTERNAL_USE_SCHEMA on it.
    ('aaaaaaaa-0000-0000-0000-00000000000a', 'creator@example.com', true, 'user', false, NULL),
    -- Usage of main.sales, MANAGE on main.sales.orders.
    ('aaaaaaaa-0000-0000-0000-00000000000b', 'manager@example.com', true, 'user', false, NULL),
    -- Owns the catalog main.
    ('aaaaaaaa-0000-0000-0000-00000000000c', 'owner@example.com', true, 'user', false, NULL),
    -- CREATE_CATALOG and USE_SHARE on the metastore.
    ('aaaaaaaa-0000-0000-0000-00000000000d', 'founder@example.com', true, 'user', false, NULL),
    -- Usage of main, SELECT and MODIFY on main.sales.
    ('aaaaaaaa-0000-0000-0000-00000000000e', 'writer@example.com', true, 'user', false, NULL),
    -- USE_CATALOG on main, SELECT and USE_SCHEMA on main.sales.orders: USE_SCHEMA
    -- on the table, not on its schema.
    ('aaaaaaaa-0000-0000-0000-00000000000f', 'misgranted@example.com', true, 'user', false, NULL),
    -- ALL_PRIVILEGES on the catalog main, and nothing else.
    ('aaaaaaaa-0000-0000-0000-000000000010', 'allpriv@example.com', true, 'user', false, NULL),
    -- An admin with EXTERNAL_USE_SCHEMA on main.sales.
    ('aaaaaaaa-0000-0000-0000-000000000011', 'admin-vendor@example.com', true, 'admin', false, NULL);

INSERT INTO authz.service_accounts (id, name, disabled_at) VALUES
    -- Usage and SELECT on main; READ_FILES and EXTERNAL_USE_LOCATION on landing.
    ('bbbbbbbb-0000-0000-0000-000000000001', 'fixture-ingest', NULL),
    -- The same grants on main, disabled.
    ('bbbbbbbb-0000-0000-0000-000000000002', 'fixture-disabled', now());

INSERT INTO authz.groups (id, name) VALUES
    ('cccccccc-0000-0000-0000-000000000001', 'fixture-analysts'),
    ('cccccccc-0000-0000-0000-000000000002', 'fixture-data');

INSERT INTO authz.memberships (group_id, principal_type, principal_id) VALUES
    ('cccccccc-0000-0000-0000-000000000001', 'user', 'aaaaaaaa-0000-0000-0000-000000000003'),
    ('cccccccc-0000-0000-0000-000000000002', 'group', 'cccccccc-0000-0000-0000-000000000001');

-- The privileges are the grants API's (unitycatalog.grants.v1), plus OWNER.
INSERT INTO authz.grants (subject_type, subject_id, privilege, object_type, object_id) VALUES
    ('group', 'cccccccc-0000-0000-0000-000000000002', 'USE_CATALOG', 'UnityCatalog::Catalog', 'main'),
    ('group', 'cccccccc-0000-0000-0000-000000000002', 'USE_SCHEMA', 'UnityCatalog::Schema', 'main.sales'),
    ('group', 'cccccccc-0000-0000-0000-000000000002', 'SELECT', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000004', 'USE_CATALOG', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000004', 'USE_SCHEMA', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000004', 'SELECT', 'UnityCatalog::Table', 'main.sales.orders'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000004', 'EXTERNAL_USE_SCHEMA', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000006', 'USE_CATALOG', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000006', 'USE_SCHEMA', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000006', 'SELECT', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000007', 'OWNER', 'UnityCatalog::Table', 'main.sales.orders'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000008', 'BROWSE', 'UnityCatalog::Metastore', 'metastore'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000009', 'USE_CATALOG', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000009', 'CREATE_SCHEMA', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000a', 'USE_CATALOG', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000a', 'USE_SCHEMA', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000a', 'CREATE_TABLE', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000a', 'EXTERNAL_USE_SCHEMA', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000b', 'USE_CATALOG', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000b', 'USE_SCHEMA', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000b', 'MANAGE', 'UnityCatalog::Table', 'main.sales.orders'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000c', 'OWNER', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000d', 'CREATE_CATALOG', 'UnityCatalog::Metastore', 'metastore'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000d', 'USE_SHARE', 'UnityCatalog::Metastore', 'metastore'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000e', 'USE_CATALOG', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000e', 'USE_SCHEMA', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000e', 'SELECT', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000e', 'MODIFY', 'UnityCatalog::Schema', 'main.sales'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000f', 'USE_CATALOG', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000f', 'USE_SCHEMA', 'UnityCatalog::Table', 'main.sales.orders'),
    ('user', 'aaaaaaaa-0000-0000-0000-00000000000f', 'SELECT', 'UnityCatalog::Table', 'main.sales.orders'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000010', 'ALL_PRIVILEGES', 'UnityCatalog::Catalog', 'main'),
    ('user', 'aaaaaaaa-0000-0000-0000-000000000011', 'EXTERNAL_USE_SCHEMA', 'UnityCatalog::Schema', 'main.sales'),
    ('service_account', 'bbbbbbbb-0000-0000-0000-000000000001', 'USE_CATALOG', 'UnityCatalog::Catalog', 'main'),
    ('service_account', 'bbbbbbbb-0000-0000-0000-000000000001', 'USE_SCHEMA', 'UnityCatalog::Catalog', 'main'),
    ('service_account', 'bbbbbbbb-0000-0000-0000-000000000001', 'SELECT', 'UnityCatalog::Catalog', 'main'),
    ('service_account', 'bbbbbbbb-0000-0000-0000-000000000002', 'USE_CATALOG', 'UnityCatalog::Catalog', 'main'),
    ('service_account', 'bbbbbbbb-0000-0000-0000-000000000002', 'USE_SCHEMA', 'UnityCatalog::Catalog', 'main'),
    ('service_account', 'bbbbbbbb-0000-0000-0000-000000000002', 'SELECT', 'UnityCatalog::Catalog', 'main'),
    -- Path credentials: READ_FILES and EXTERNAL_USE_LOCATION on the external location.
    ('service_account', 'bbbbbbbb-0000-0000-0000-000000000001', 'READ_FILES', 'UnityCatalog::ExternalLocation', 'landing'),
    ('service_account', 'bbbbbbbb-0000-0000-0000-000000000001', 'EXTERNAL_USE_LOCATION', 'UnityCatalog::ExternalLocation', 'landing');

COMMIT;
