-- What the lakehouse adds to the shared `authz` schema, after olai-auth's
-- migrations (`olai_auth::migrate`, which creates `authz.service_accounts`,
-- `api_keys`, `groups`, `memberships`, `grants` and the `authz_reader` role).
--
-- Its ledger is `luminescent._sqlx_migrations`, so it doesn't mix with
-- olai-auth's (`authz._sqlx_migrations`); its objects live in `authz`.

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
-- olai-auth's default privileges cover tables its role creates; grant this one
-- explicitly in case the migrating role differs.
GRANT SELECT ON authz.github_oidc_trust TO authz_reader;

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
