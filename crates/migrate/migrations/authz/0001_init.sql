-- The permission data every function on a branch reads: who the machine
-- callers are, how they authenticate, and what anyone may do. Neon Auth owns
-- the users (`neon_auth."user"`); this schema refers to them by id.
--
-- Principals are typed: `user` (a Neon Auth user id), `service_account` and
-- `group` (ids from the tables below). Objects are Cedar entities: an entity
-- type such as `Headwaters::Namespace` and an id. Relations are free text that
-- the Cedar policies give meaning to (`reader`, `writer`, ...).

CREATE TABLE authz.service_accounts (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    name        text        NOT NULL UNIQUE CHECK (name ~ '^[a-z0-9][a-z0-9._-]{0,62}$'),
    description text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    -- A disabled account authenticates with nothing: its keys and its GitHub
    -- trust rules stop working.
    disabled_at timestamptz
);

-- `olk_<id>_<secret>`: `id` is public (16 hex characters, the lookup key),
-- the secret is 32 random bytes as hex. Only the SHA-256 of the whole key
-- string is stored; the key itself is shown once, when it is issued.
CREATE TABLE authz.api_keys (
    id          text        PRIMARY KEY CHECK (id ~ '^[0-9a-f]{16}$'),
    account_id  uuid        NOT NULL REFERENCES authz.service_accounts (id) ON DELETE CASCADE,
    sha256      bytea       NOT NULL CHECK (length(sha256) = 32),
    scopes      text[]      NOT NULL DEFAULT '{}',
    description text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz,
    revoked_at  timestamptz
);
CREATE INDEX api_keys_account ON authz.api_keys (account_id);

-- Which GitHub Actions OIDC tokens act as a service account. GitHub's numeric
-- ids, never names: a renamed or re-registered owner or repository would
-- otherwise inherit the trust. A NULL column matches any value.
CREATE TABLE authz.github_oidc_trust (
    id                  uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    account_id          uuid        NOT NULL REFERENCES authz.service_accounts (id) ON DELETE CASCADE,
    repository_owner_id bigint      NOT NULL,
    repository_id       bigint,
    -- A glob over the `ref` claim, e.g. `refs/heads/main` or `refs/tags/v*`.
    ref_pattern         text,
    environment         text,
    created_at          timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX github_oidc_trust_owner ON authz.github_oidc_trust (repository_owner_id);

CREATE TABLE authz.groups (
    id          uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    name        text        NOT NULL UNIQUE CHECK (name ~ '^[a-z0-9][a-z0-9._-]{0,62}$'),
    description text,
    created_at  timestamptz NOT NULL DEFAULT now()
);

-- Groups can contain groups; the Cedar entity loader walks the parents.
CREATE TABLE authz.memberships (
    group_id       uuid        NOT NULL REFERENCES authz.groups (id) ON DELETE CASCADE,
    principal_type text        NOT NULL CHECK (principal_type IN ('user', 'service_account', 'group')),
    principal_id   text        NOT NULL,
    created_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (group_id, principal_type, principal_id),
    CHECK (NOT (principal_type = 'group' AND principal_id = group_id::text))
);
CREATE INDEX memberships_principal ON authz.memberships (principal_type, principal_id);

-- (subject, relation, object): the permission tuples.
CREATE TABLE authz.grants (
    id           uuid        PRIMARY KEY DEFAULT gen_random_uuid(),
    subject_type text        NOT NULL CHECK (subject_type IN ('user', 'service_account', 'group')),
    subject_id   text        NOT NULL,
    relation     text        NOT NULL CHECK (relation ~ '^[a-z][a-z0-9_]{0,62}$'),
    object_type  text        NOT NULL CHECK (object_type ~ '^[A-Z][A-Za-z0-9_]*(::[A-Z][A-Za-z0-9_]*)+$'),
    object_id    text        NOT NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    UNIQUE (subject_type, subject_id, relation, object_type, object_id)
);
CREATE INDEX grants_object ON authz.grants (object_type, object_id);
CREATE INDEX grants_subject ON authz.grants (subject_type, subject_id);

-- `authz_reader`: what the functions need to authenticate and authorize, and
-- no more. A login role the functions connect as joins it (`GRANT authz_reader
-- TO ...`). Roles are per cluster, so this one may exist already: another
-- database's migration, or the parent branch.
DO $$
BEGIN
    IF NOT EXISTS (SELECT FROM pg_roles WHERE rolname = 'authz_reader') THEN
        CREATE ROLE authz_reader NOLOGIN;
    END IF;
END
$$;
GRANT USAGE ON SCHEMA authz TO authz_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA authz TO authz_reader;
ALTER DEFAULT PRIVILEGES IN SCHEMA authz GRANT SELECT ON TABLES TO authz_reader;

-- Neon Auth's users: the role and ban status the services enforce, and the
-- linked GitHub account id. Never emails, sessions, tokens or password hashes.
-- Neon Auth is on for every Neon branch; a plain Postgres (local tests) has no
-- `neon_auth` schema, so there is nothing to grant.
DO $$
BEGIN
    IF to_regclass('neon_auth."user"') IS NOT NULL THEN
        GRANT USAGE ON SCHEMA neon_auth TO authz_reader;
        GRANT SELECT (id, role, banned, "banExpires", "emailVerified") ON neon_auth."user" TO authz_reader;
        GRANT SELECT ("userId", "providerId", "accountId") ON neon_auth.account TO authz_reader;
    ELSE
        RAISE NOTICE 'no neon_auth schema: authz_reader gets no access to Neon Auth users';
    END IF;
END
$$;
