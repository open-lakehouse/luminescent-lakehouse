# Per-function build/test settings for the justfile recipes.
#
# Usage: `source scripts/function.sh <fn>`. Sets:
#   CRATE      the napi addon crate
#   LIB        its cdylib file stem
#   PROFILE    the cargo profile of the deployable build (see Cargo.toml)
#   NODE       the addon name `function/index.ts` loads from `native/`
#   NEEDS_PG   1 if the tests need a migrated Postgres
#   TESTS      the test command, run inside the function directory
#
# To host another service, add a case here and a crate under crates/.

case "${1:?usage: source scripts/function.sh <function>}" in
headwaters)
    CRATE=headwaters-neon
    LIB=libheadwaters_neon
    PROFILE=release-neon
    NODE=headwaters.node
    NEEDS_PG=1
    TESTS='node --test smoke.test.mjs && HEADWATERS_NEON_SETTLE_ON_INGEST=false node --test lazy.test.mjs'
    ;;
opensharing)
    CRATE=opensharing-neon
    LIB=libopensharing_neon
    # ~11 MiB zipped at opt-level 3, over Neon's limit.
    PROFILE=release-neon-small
    NODE=sharing.node
    NEEDS_PG=0
    # The in-memory store.
    TESTS='node --test smoke.test.mjs'
    ;;
*)
    echo "error: unknown function '$1' (headwaters, opensharing)" >&2
    exit 1
    ;;
esac

FN_DIR="crates/$CRATE/function"
ZIP="crates/$CRATE/$CRATE.zip"
