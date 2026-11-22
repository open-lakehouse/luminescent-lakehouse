## Task T2: Build the core on olai-store
ref `olai:019ab3c2-5b1e-7f00-8000-000000000002` · parent plan `olai:019ab3c2-5b1e-7f00-8000-000000000000` · approved · Ready

Writes go through the schema.

**Task**

- **acceptance:**
  Writes are validated against the schema,
  and every change records a revision.
- **profile:** executor
- **repo:** open-lakehouse/luminescent-lakehouse
- **touchedPaths:** `crates/factory-core/**`

**Labels:** `area=core`

**Edges**

- `blocked_by` T1 `olai:019ab3c2-5b1e-7f00-8000-000000000001`
- T3 `olai:019ab3c2-5b1e-7f00-8000-000000000003` `blocked_by` this item
- G6 `olai:019ab3c2-5b1e-7f00-8000-000000000006` `blocked_by` this item

**Annotations** (1)

- Learning `olai:ann-1` by factory-planner
  - **evidence:** `PR #14`
  - **lesson:** Index edges by kind before ranking.

**Runs** (1)

| run | attempt | executor | state | stage | cost | artifacts |
|---|---|---|---|---|---|---|
| `olai:run-1` | 1 | task.v1 | completed | done | $1.25 | [open-lakehouse/luminescent-lakehouse#14](https://github.com/open-lakehouse/luminescent-lakehouse/pull/14) (merged) |

**Attention** (1)

- proposal `olai:att-1` (accept by operator: Go ahead.): Split the core into two tasks
