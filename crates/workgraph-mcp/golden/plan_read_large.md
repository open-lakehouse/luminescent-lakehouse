## Plan: Migrate every service to olai-store
ref `olai:019ab3c2-5b1e-7f00-8000-000000000000` · approved

**Goal:** An approved plan runs end to end on olai-store, with no GitHub issues.

**Rubric:**
- The conformance suite passes on every backend.
- Every revision names its actor.

**Budget:** $120.00

**Revision** `4be1d0c39a7f25e8b1c6d4f0a9e37b2c5d8f1a6e0b3c7d9e2f4a1b5c8d0e3f6a` · **approved** `a17c9e2d4f6b8a0c3e5d7f9b1a3c5e7d9f0b2a4c6e8d0f1a3b5c7e9d2f4a6b8c`

Graph left out: 48 items, more than 40. Read a part with `plan_read` and `under: <ref>`; the full graph is in the control room: http://localhost:4330/?plan=olai%3A019ab3c2-5b1e-7f00-8000-000000000000.

**Items** (48)

| # | ref | kind | title | phase | conditions | blocked_by | parent |
|---|---|---|---|---|---|---|---|
| T1 | `olai:019ab3c2-5b1e-7f00-8000-000000000001` | Task | Catalog service | approved | – | – | plan |
| T2 | `olai:019ab3c2-5b1e-7f00-8000-000000000002` | Task | Catalog: migrate step 1 | done | – | – | T1 |
| T3 | `olai:019ab3c2-5b1e-7f00-8000-000000000003` | Task | Catalog: migrate step 2 | done | – | T2 | T1 |
| T4 | `olai:019ab3c2-5b1e-7f00-8000-000000000004` | Task | Catalog: migrate step 3 | approved | Ready | T3 | T1 |
| T5 | `olai:019ab3c2-5b1e-7f00-8000-000000000005` | Task | Catalog: migrate step 4 | approved | Blocked | T4 | T1 |
| T6 | `olai:019ab3c2-5b1e-7f00-8000-000000000006` | Task | Catalog: migrate step 5 | approved | Blocked | T5 | T1 |
| T7 | `olai:019ab3c2-5b1e-7f00-8000-000000000007` | Task | Catalog: migrate step 6 | approved | Blocked | T6 | T1 |
| T8 | `olai:019ab3c2-5b1e-7f00-8000-000000000008` | Task | Catalog: migrate step 7 | approved | Blocked | T7 | T1 |
| T9 | `olai:019ab3c2-5b1e-7f00-8000-000000000009` | Task | Catalog: migrate step 8 | approved | Blocked | T8 | T1 |
| T10 | `olai:019ab3c2-5b1e-7f00-8000-000000000010` | Task | Catalog: migrate step 9 | approved | Blocked | T9 | T1 |
| T11 | `olai:019ab3c2-5b1e-7f00-8000-000000000011` | Task | Catalog: migrate step 10 | approved | Blocked | T10 | T1 |
| T12 | `olai:019ab3c2-5b1e-7f00-8000-000000000012` | Task | Catalog: migrate step 11 | approved | Blocked | T11 | T1 |
| T13 | `olai:019ab3c2-5b1e-7f00-8000-000000000013` | Task | Catalog: migrate step 12 | approved | Blocked | T12 | T1 |
| T14 | `olai:019ab3c2-5b1e-7f00-8000-000000000014` | Task | Catalog: migrate step 13 | approved | Blocked | T13 | T1 |
| T15 | `olai:019ab3c2-5b1e-7f00-8000-000000000015` | Task | Catalog: migrate step 14 | approved | Blocked | T14 | T1 |
| T16 | `olai:019ab3c2-5b1e-7f00-8000-000000000016` | Task | Catalog: migrate step 15 | approved | Blocked | T15 | T1 |
| T17 | `olai:019ab3c2-5b1e-7f00-8000-000000000017` | Task | Sharing service | approved | – | – | plan |
| T18 | `olai:019ab3c2-5b1e-7f00-8000-000000000018` | Task | Sharing: migrate step 1 | done | – | – | T17 |
| T19 | `olai:019ab3c2-5b1e-7f00-8000-000000000019` | Task | Sharing: migrate step 2 | done | – | T18 | T17 |
| T20 | `olai:019ab3c2-5b1e-7f00-8000-000000000020` | Task | Sharing: migrate step 3 | done | – | T19 | T17 |
| T21 | `olai:019ab3c2-5b1e-7f00-8000-000000000021` | Task | Sharing: migrate step 4 | done | – | T20 | T17 |
| T22 | `olai:019ab3c2-5b1e-7f00-8000-000000000022` | Task | Sharing: migrate step 5 | done | – | T21 | T17 |
| T23 | `olai:019ab3c2-5b1e-7f00-8000-000000000023` | Task | Sharing: migrate step 6 | approved | Ready | T22 | T17 |
| T24 | `olai:019ab3c2-5b1e-7f00-8000-000000000024` | Task | Sharing: migrate step 7 | approved | Blocked | T23 | T17 |
| T25 | `olai:019ab3c2-5b1e-7f00-8000-000000000025` | Task | Sharing: migrate step 8 | approved | Blocked | T24 | T17 |
| T26 | `olai:019ab3c2-5b1e-7f00-8000-000000000026` | Task | Sharing: migrate step 9 | approved | Blocked | T25 | T17 |
| T27 | `olai:019ab3c2-5b1e-7f00-8000-000000000027` | Task | Sharing: migrate step 10 | approved | Blocked | T26 | T17 |
| T28 | `olai:019ab3c2-5b1e-7f00-8000-000000000028` | Task | Sharing: migrate step 11 | approved | Blocked | T27 | T17 |
| T29 | `olai:019ab3c2-5b1e-7f00-8000-000000000029` | Task | Sharing: migrate step 12 | approved | Blocked | T28 | T17 |
| T30 | `olai:019ab3c2-5b1e-7f00-8000-000000000030` | Task | Sharing: migrate step 13 | approved | Blocked | T29 | T17 |
| T31 | `olai:019ab3c2-5b1e-7f00-8000-000000000031` | Task | Sharing: migrate step 14 | approved | Blocked | T30 | T17 |
| T32 | `olai:019ab3c2-5b1e-7f00-8000-000000000032` | Task | Sharing: migrate step 15 | approved | Blocked | T31 | T17 |
| T33 | `olai:019ab3c2-5b1e-7f00-8000-000000000033` | Task | Lineage service | approved | – | – | plan |
| T34 | `olai:019ab3c2-5b1e-7f00-8000-000000000034` | Task | Lineage: migrate step 1 | done | – | – | T33 |
| T35 | `olai:019ab3c2-5b1e-7f00-8000-000000000035` | Task | Lineage: migrate step 2 | done | – | T34 | T33 |
| T36 | `olai:019ab3c2-5b1e-7f00-8000-000000000036` | Task | Lineage: migrate step 3 | done | – | T35 | T33 |
| T37 | `olai:019ab3c2-5b1e-7f00-8000-000000000037` | Task | Lineage: migrate step 4 | done | – | T36 | T33 |
| T38 | `olai:019ab3c2-5b1e-7f00-8000-000000000038` | Task | Lineage: migrate step 5 | done | – | T37 | T33 |
| T39 | `olai:019ab3c2-5b1e-7f00-8000-000000000039` | Task | Lineage: migrate step 6 | done | – | T38 | T33 |
| T40 | `olai:019ab3c2-5b1e-7f00-8000-000000000040` | Task | Lineage: migrate step 7 | done | – | T39 | T33 |
| T41 | `olai:019ab3c2-5b1e-7f00-8000-000000000041` | Task | Lineage: migrate step 8 | approved | Ready | T40 | T33 |
| T42 | `olai:019ab3c2-5b1e-7f00-8000-000000000042` | Task | Lineage: migrate step 9 | approved | Blocked | T41 | T33 |
| T43 | `olai:019ab3c2-5b1e-7f00-8000-000000000043` | Task | Lineage: migrate step 10 | approved | Blocked | T42 | T33 |
| T44 | `olai:019ab3c2-5b1e-7f00-8000-000000000044` | Task | Lineage: migrate step 11 | approved | Blocked | T43 | T33 |
| T45 | `olai:019ab3c2-5b1e-7f00-8000-000000000045` | Task | Lineage: migrate step 12 | approved | Blocked | T44 | T33 |
| T46 | `olai:019ab3c2-5b1e-7f00-8000-000000000046` | Task | Lineage: migrate step 13 | approved | Blocked | T45 | T33 |
| T47 | `olai:019ab3c2-5b1e-7f00-8000-000000000047` | Task | Lineage: migrate step 14 | approved | Blocked | T46 | T33 |
| T48 | `olai:019ab3c2-5b1e-7f00-8000-000000000048` | Task | Lineage: migrate step 15 | approved | Blocked | T47 | T33 |
