# Per-function build/test settings for the justfile recipes.
#
# Usage: `source scripts/function.sh <fn>`. Sets:
#   CRATE      the napi addon crate
#   LIB        its cdylib file stem
#   PROFILE    the cargo profile of the deployable build (see Cargo.toml)
#   NODE       the addon name `function/index.ts` loads from `native/`
#   NEEDS_PG   1 if it has migrations (scripts/migrate.sh runs them before
#              every deploy), so its tests need a migrated Postgres
#   DATABASE   the database it lives in on a Neon branch (created when missing)
#   SEED       SQL the tests need in the test database, applied after the
#              `authz` migration; empty for none
#   SECRETS    the env vars a deployer sets once per branch, never in neon.ts
#              (docs/deployment.md, "Secrets"); listed in the release manifest
#   TESTS      the test command, run inside the function directory
#   STAGE      optional: a command run (with FN_DIR set) once dist/ is staged
#
# To host another service, add a case here and a crate under crates/.

STAGE=""

case "${1:?usage: source scripts/function.sh <function>}" in
headwaters)
    CRATE=headwaters-neon
    LIB=libheadwaters_neon
    PROFILE=release-neon
    NODE=headwaters.node
    NEEDS_PG=1
    DATABASE=neondb
    SEED=""
    SECRETS=""
    TESTS='node --test smoke.test.mjs && HEADWATERS_NEON_SETTLE_ON_INGEST=false node --test lazy.test.mjs'
    ;;
opensharing)
    CRATE=opensharing-neon
    LIB=libopensharing_neon
    # ~11 MiB zipped at opt-level 3, over Neon's limit.
    PROFILE=release-neon-small
    NODE=sharing.node
    NEEDS_PG=0
    DATABASE=neondb
    SEED=""
    # Not in neon.ts yet; its KEKs join these then (docs/deployment.md).
    SECRETS="SHARING_SERVER_CONFIG_YAML SHARING_PROVIDER_TOKEN"
    # The in-memory store.
    TESTS='node --test smoke.test.mjs'
    ;;
unitycatalog)
    CRATE=unitycatalog-neon
    LIB=libunitycatalog_neon
    # 7.47 MiB zipped at opt-level 3 with olai-auth, under Neon's 10 MiB.
    PROFILE=release-neon
    NODE=unitycatalog.node
    # In its own `unitycatalog` database (see crates/unitycatalog-neon/README.md),
    # which `just pg-up` creates next to the test database.
    NEEDS_PG=1
    DATABASE=unitycatalog
    # Its callers (smoke.test.mjs): Neon Auth users and an API key.
    SEED=crates/unitycatalog-neon/function/fixtures/identity.sql
    # The key-encryption key (crates/unitycatalog-neon/README.md, "Secrets").
    SECRETS="UC_SERVER_KEK"
    TESTS='node --test smoke.test.mjs'
    ;;
authz)
    CRATE=authz-neon
    LIB=libauthz_neon
    # cedar-policy adds ~2.0 MB zipped at opt-level 3; it fits easily.
    PROFILE=release-neon
    NODE=authz.node
    # Reads `authz` and `neon_auth` in the branch's main database; its
    # migrations are `luminescent-migrate authz`, which every deploy runs first.
    NEEDS_PG=1
    DATABASE=neondb
    # One per calling service (docs/deployment.md, "Secrets").
    SECRETS="AUTHZ_TOKEN_UNITYCATALOG AUTHZ_TOKEN_HEADWATERS AUTHZ_TOKEN_OPENSHARING"
    # The fixture principals, groups and grants (and a stand-in neon_auth).
    SEED=crates/authz-neon/function/seed.sql
    # The policies and the upstream schema fragments.
    STAGE='scripts/authz-policies.sh "$FN_DIR/dist/policies"'
    TESTS='node --test smoke.test.mjs fixtures.test.mjs loader-error.test.mjs invalid-policy.test.mjs'
    ;;
*)
    echo "error: unknown function '$1' (headwaters, opensharing, unitycatalog, authz)" >&2
    exit 1
    ;;
esac

FN_DIR="crates/$CRATE/function"
ZIP="crates/$CRATE/$CRATE.zip"
