"""Normalize Omnigent's openapi.json for tests/contract.rs.

Upstream's file holds raw control characters in strings, which strict JSON (serde_json)
rejects; read it leniently and write it back escaped, with sorted keys.

Usage: python3 -I normalize-openapi.py <in> <out>
"""

import json
import sys

with open(sys.argv[1], encoding="utf-8") as f:
    spec = json.load(f, strict=False)
with open(sys.argv[2], "w", encoding="utf-8") as f:
    json.dump(spec, f, indent=1, sort_keys=True, ensure_ascii=True)
    f.write("\n")
