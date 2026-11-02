// Prints the functions neon.ts deploys, one slug per line, or as a JSON array
// with `--json` (the CI deploy matrix). A slug is also the function's name in
// scripts/function.sh.
//
// Needs Node 24 (type stripping) and `npm ci` (neon.ts imports @neon/config).

import config from "../neon.ts";

const slugs = Object.keys(config.functions ?? {});
console.log(process.argv.includes("--json") ? JSON.stringify(slugs) : slugs.join("\n"));
