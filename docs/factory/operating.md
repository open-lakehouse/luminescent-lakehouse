# Operating runs: asks, sessions and people stepping in

Status: design, not built. [execution.md](execution.md) covers how a plan
runs. This doc covers what a person does while it runs, and what the factory
does around them:

- answering an agent's question;
- what happens to a run's Omnigent sessions while it waits and when it ends;
- a person working in a run's session directly;
- what accepting a proposal does.

The first task on managed sandboxes (mangrove#372 → #390,
[hosts.md](hosts.md#setup)) needed an operator at every step, and each step
was done through `factory.v1` by hand. That run found:

- push approvals, a false positive, two `AskUserQuestion` questions and a
  proposal, all resolved with `ResolveAttention`;
- a sub-agent's ask raised on its parent's session, where resolving it didn't
  reach the sub-agent (#264);
- a decline's comment lost: the agent read only "Tool call denied by user";
- canceled and retried attempts leaving their sessions running, and about ten
  idle sessions left behind;
- an accepted proposal adding nothing to the plan (#266).

Checked against the Omnigent server source on 2026-11-28. The behaviour
marked **(spike)** is read from the source and still has to be confirmed on
hosted (the first item in [Work](#work)).

## What Omnigent offers

| Need | Omnigent | Notes |
|---|---|---|
| Where an ask lives | `response.elicitation_request` on the asking session's stream, mirrored to every ancestor with `params.target_session_id` set to the asking session. A parent's snapshot lists its descendants' pending prompts the same way. | Resolve on the target, never the parent. |
| Answering | `POST /v1/sessions/{target}/elicitations/{id}/resolve` with `{action: accept\|decline\|cancel, content?}`. There is no comment field. | Only `accept` approves. |
| `AskUserQuestion` | The request carries the questions and options (`ask_user_question`, plus `requestedSchema`). An accept's `content` is `{<question text>: <label>}`, or a list of labels for a multi-select question, and the harness hands it to Claude as the tool's `answers`. | So the inbox can answer it. |
| Free text on a decline | Only `content.feedback` on the Claude-native permission path becomes the denial's message. A declined policy ASK, which is what our push gates are, interrupts the turn and returns the policy's reason. **(spike)** | So a comment goes as a message. |
| Children | `GET /v1/sessions/{id}/child_sessions?descendants=true`, plus `parent_session_id` and `root_conversation_id` on every session. | |
| Stopping | `interrupt` (edit) ends the turn. `stop_session` (owner) kills the runner and keeps the transcript, and the next message relaunches it. Archive is `PATCH {archived: true}` (owner), which stops the runner after an undo grace. | A session's owner is its creator, which is the factory's principal. |
| Idle sandboxes | A managed sandbox suspends after the deployment's `keep_warm_s`. A message to a session whose sandbox is gone provisions a new sandbox generation for the same host and re-clones the workspace. **(spike: hosted's value, and whether `stop_session` frees the sandbox at once)** | What was pushed survives. Uncommitted work doesn't. |
| Who did what | Every user item and `session.input.consumed` carry `created_by`, the human author. Agent, tool and system items have none. The `omnigent.turn_actor` label is set by each human-forwarded message. `session.interrupted` and `response.elicitation_resolved` carry no actor. | A person's message is visible. A person's interrupt has to be inferred. |
| Who may act | `PUT /v1/sessions/{id}/permissions`. The factory shares its sessions with the operators at `manage`, so a person can message, interrupt and answer, but not stop or archive. | |

## Answering an agent's question

### Raising it on the right session

The reconciler follows each active step's session. An ask on that stream, or
in its snapshot, carries `target_session_id`, which is the step's session or one
of its descendants. The `ask` item records both:

- `target_session_id`: the session the ask belongs to, where it's resolved;
- `holder_session_id`: the step's session, which the run follows.

Dedupe is on (target, elicitation id). The item also keeps what the inbox
needs in order to render it: the mode, the requested schema, the questions
with their options, the policy name and the content preview.

### Delivering the reply

`ResolveAttention` gains `content`, a `Struct`:

| Resolution | Sent to the target | Then |
|---|---|---|
| accept an `AskUserQuestion` | `accept`, `content` = the answers by question | |
| accept a form | `accept`, `content` = the form | |
| accept a permission (push) | `accept` | |
| decline | `decline`, `content` = `{feedback: <comment>}` when there is one | |
| dismiss | `cancel` | |
| any, with a comment | | a user message to the target: `Operator on <tool>: <comment>` |

The message is what makes the comment reach the agent on every path. On an
accept it steers the turn that continues. On a declined policy ASK, which ends
the turn, it starts the next one. When the target is the step's own session, it
counts as a follow-up, so the executor waits for that turn before it reads the
session as done. When the target is a sub-agent, the parent keeps waiting on its
inbox as it does today.

Two more rules close the loops the first task left open:

- **An ask answered elsewhere.** A person with `manage` can answer in
  Omnigent's own UI. When an ask leaves the target's pending list without the
  factory resolving it, its item is resolved with no `resolved_by`, marked
  applied, and nothing is forwarded.
- **An ask on a finished run.** When a run finishes, its open asks are
  dismissed and marked applied. Today a resolution sent to a session that is
  gone fails on every pass.

## Sessions while a run waits, and after it ends

A run's sessions and sandboxes outlive its steps unless the factory ends them.
The rule: **a session gives up its sandbox when nothing waits on it, and is
archived when its run ends.**

| When | The factory |
|---|---|
| a reviewer posts its verdict | stops the reviewer's session and its descendants |
| the run reaches `awaiting_merge`, and gives its place back | stops the implementing session and its descendants |
| a fix round or retry messages a stopped session | nothing more: the message relaunches it on a new sandbox |
| the run is completed, failed or canceled | interrupts whatever still runs, then archives every step's session and every descendant |
| `RetryRun` on a live run | the reconciler interrupts and archives the old attempt's sessions, then queues the next attempt |
| the server starts | archives the unarchived sessions of runs that aren't live (an idempotent sweep that cleans up after a crash) |

Stopping rather than archiving keeps the session's transcript and its place in
Omnigent's UI, and its PRs stay linked. A relaunched implementer starts on a
fresh clone. So the fix-round message says which branch to check out, and the
setup skill fetches it. If `stop_session` doesn't free a managed sandbox at
once **(spike)**, the factory still stops it, which ends the runner, and the
sandbox suspends after `keep_warm_s`.

Each step's session payload gains:

- `children`: id, sub-agent name, status, UI link, stopped and archived, refreshed
  whenever the session is followed;
- `stopped` and `archived` for the session itself;
- `factory_interrupt_at`, when the factory last interrupted it.

The run view lists the children under their parent.

## A person steps in: the paused run

Operators can message and interrupt a run's sessions in Omnigent. When they do,
the reconciler must not retry, restart or move on as if the agent had
finished. So the run **pauses**.

### Detecting it

For each followed session and its descendants:

| Signal | How it's seen |
|---|---|
| a message | an item or `session.input.consumed` whose `created_by` is set and isn't the author of the session's initial input, which is the factory's own identity. That needs no configuration. A parent's message to its child is an agent item and has no `created_by` **(spike)**. |
| an interrupt | `session.interrupted`, or a turn that ends `incomplete` with reason `user_interrupt`, with no factory interrupt in the 60 s before it (`factory_interrupt_at`). After a dropped stream, the snapshot's last response shows it. |
| a stop or archive | a session the factory didn't stop or archive that is stopped or archived. Owner-only today, so only the factory can do it, but it's checked in case hosted opens it up. |

### The `paused` state

`RunState` gains `PAUSED`. `Run` gains `pause`: the signal (`message`,
`interrupt`, `archive`, `stop` or `operator`), the actor when it's known, the
session, when, and a short detail. A new attention kind, `intervention`, carries
the same record.

```
QUEUED ─► WORKING ⇄ INPUT_REQUIRED
            │            │
            └──► PAUSED ◄┘   a person stepped in, or PauseRun
                   │
                   ├─► WORKING    ResumeRun, or resolve the intervention with resume
                   └─► CANCELED   CancelRun, or resolve with cancel
COMPLETED / FAILED / CANCELED ─► sessions archived
```

While a run is paused:

- it stays live, so nothing else is dispatched to its node, and it keeps its
  place in its environment;
- the reconciler keeps following its sessions, raising their asks and updating
  their status and cost;
- it doesn't call the executor, so the run neither stalls, retries nor advances;
- `drive`'s state rule leaves `PAUSED` alone.

When the run resumes, the executor reads the sessions as they are now. If the
person got the implementer to open its PR, review starts. If they stopped it
with no PR, the usual stall rule applies. Another intervention while the run is
paused updates the same item and doesn't raise a new one.

`PauseRun` lets an operator pause from the control room without touching a
session (signal `operator`). Neither `PauseRun` nor `ResumeRun` touches the
sessions themselves.

## Accepted proposals

An agent's `propose` today records text, and accepting it changes nothing
(#266). The design:

- `propose` takes an optional **task draft**: title, repo, body and base
  branch. The `proposal` item's detail becomes `ProposalDetail{text, draft}`. Old
  items, which have only the text, still read.
- **Accepting a proposal with a draft adds the task.** The reconciler
  creates a Task node from the draft, or from the edited draft in the
  resolution's `content`, under the proposer's parent and after the proposer's
  node. That moves the plan's revision, so the amendment policies assess it as
  usual. The default accepts a Task on one of the plan's repos, and the reconciler
  approves the revision and dispatches the task. Accepting is the approval. A
  Task on a repo outside the plan still raises an `approval`.
- The new node and any comment go into the planner's next stage report, if
  the plan has a planner. A declined proposal's comment goes there too.
- Accepting a proposal with no draft adds nothing. The item says so.

## API changes

| Where | Change |
|---|---|
| `AskDetail` | `target_session_id`, `holder_session_id` (the old `session_id` reads as both), `mode`, `requested_schema`, `questions` (`AskQuestion{question, header, multi_select, options[{label, description}]}`), `content_preview` |
| `Attention`, `ResolveAttentionRequest` | `content` (`google.protobuf.Struct`) |
| `Resolution` | `RESUME`, `CANCEL` |
| `RunState` | `PAUSED` |
| `Run` | `pause` (`PauseInfo`) |
| `AttentionKind` | `INTERVENTION`, with `PauseInfo` as its detail |
| `OmnigentSession` | `children` (`SubSession`), `stopped`, `archived`, `factory_interrupt_at` |
| `proposal` detail | `ProposalDetail{text, draft: TaskDraft}` |
| `factory.v1` | `PauseRun(run, reason)`, `ResumeRun(run)` |
| `OmnigentDriver` | `resolve_ask` sends content, `stop`, `archive`, `children`, and the human-authored items since a cursor |
| workgraph-mcp | `propose` takes `task` |

New enum values are appended, so older readers keep unknown values.

## Where the control room reads it

| View | Reads |
|---|---|
| Inbox | `ListAttention` across roots. Each item by kind: an ask's questions or schema as a form, a proposal's draft as an editable form, a pause's signal and actor. The actions are the resolutions above, each with a comment. |
| Run | `GetRun`: the stage, the state and `pause`, its steps with each session's status, cost, UI link and children, the PRs, and `ListPlaces` while it waits. Pause, resume, retry and cancel. |
| Live | `WatchGraph` for the run's root, which already carries changed runs and attention items. The inbox spans every root, so it refetches on a timer and on focus. |

## Work

1. **Spike on hosted (factory-staging).** Confirm what the design marks
   **(spike)**:
   - `content.feedback` and the follow-up message reaching Claude after a declined push;
   - `AskUserQuestion` answers;
   - who owns a sub-agent's session;
   - whether a parent's message to its child carries `created_by`;
   - whether `stop_session` frees a managed sandbox, and whether a message relaunches it;
   - how an operator's interrupt shows in the snapshot.
2. **Session lifecycle**: stop, archive, children, `RetryRun` through the
   reconciler, the start-up sweep. It comes first because the sweep archives
   the sessions earlier runs left behind, on the first deploy that has it.
3. **Asks** (#264): routing, content, the comment message, asks answered
   elsewhere, asks on finished runs.
4. **Paused runs**: detection, `PAUSED`, `intervention`, `PauseRun` and
   `ResumeRun`.
5. **Accepted proposals** (#266): the draft, and accepting it as an amendment.
6. **The control room** in factory-ui: inbox, run view and submitting a one-task
   plan, released for olai-sigint's Factory pages.

2–4 build on each other and land as one stack. 5 and 6 are their own PRs, and
6 follows the rest.
