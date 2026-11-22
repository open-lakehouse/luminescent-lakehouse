## Plan: Fan out the connectors
ref `olai:019ab3c2-5b1e-7f00-8000-000000000000` · approved

**Goal:** An approved plan runs end to end on olai-store, with no GitHub issues.

**Rubric:**
- The conformance suite passes on every backend.
- Every revision names its actor.

**Revision** `4be1d0c39a7f25e8b1c6d4f0a9e37b2c5d8f1a6e0b3c7d9e2f4a1b5c8d0e3f6a` · never approved

Graph collapsed to one node per phase: its widest rank has 6 items (more than 4), and it's 6 ranks deep, too deep to turn. Read a part with `plan_read` and `under: <ref>`; the full graph is in the control room.

```mermaid
flowchart LR
  classDef proposed fill:#f5f5f5,stroke:#9e9e9e,stroke-dasharray:4 3
  classDef approved fill:#e3f2fd,stroke:#1e88e5
  classDef done fill:#e8f5e9,stroke:#43a047
  phase_proposed["proposed: 4 items"]:::proposed
  phase_approved["approved: 6 items"]:::approved
  phase_done["done: 1 item"]:::done
  phase_approved --> phase_proposed
  phase_done --> phase_approved
```

**Items** (11)

| # | ref | kind | title | phase | conditions | blocked_by |
|---|---|---|---|---|---|---|
| T1 | `olai:019ab3c2-5b1e-7f00-8000-000000000001` | Task | Shared connector trait | done | – | – |
| T2 | `olai:019ab3c2-5b1e-7f00-8000-000000000002` | Task | Postgres connector | approved | Ready | T1 |
| T3 | `olai:019ab3c2-5b1e-7f00-8000-000000000003` | Task | MySQL connector | approved | Ready | T1 |
| T4 | `olai:019ab3c2-5b1e-7f00-8000-000000000004` | Task | SQLite connector | approved | Ready | T1 |
| T5 | `olai:019ab3c2-5b1e-7f00-8000-000000000005` | Task | DuckDB connector | approved | Ready | T1 |
| T6 | `olai:019ab3c2-5b1e-7f00-8000-000000000006` | Task | Snowflake connector | approved | Ready | T1 |
| T7 | `olai:019ab3c2-5b1e-7f00-8000-000000000007` | Task | BigQuery connector | approved | Ready | T1 |
| T8 | `olai:019ab3c2-5b1e-7f00-8000-000000000008` | Task | Connector conformance suite | proposed | Blocked | T2, T3, T4, T5, T6, T7 |
| T9 | `olai:019ab3c2-5b1e-7f00-8000-000000000009` | Task | Step 9 | proposed | Blocked | T8 |
| T10 | `olai:019ab3c2-5b1e-7f00-8000-000000000010` | Task | Step 10 | proposed | Blocked | T9 |
| T11 | `olai:019ab3c2-5b1e-7f00-8000-000000000011` | Task | Step 11 | proposed | Blocked | T10 |
