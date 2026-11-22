//! The text of the tools' results: compact markdown a model can reason over
//! and relay into its reply.
//!
//! An agent host such as Omnigent shows a tool's result to the model only as
//! its flattened text, and its tool cards show that text raw: markdown,
//! tables and Mermaid render only once the model relays them. So each result
//! is a short header, at most one Mermaid fence, and a table, and it carries
//! everything the model needs from the structured result (refs, kinds,
//! titles, phases, conditions, edges, revisions). The JSON stays in
//! `structuredContent` for clients that read it.
//!
//! **Aliases.** Refs are long, so a rendering names each item by its kind's
//! initial and a number (`T3`), in the plan's outline order. Aliases are only
//! for reading one result: the tables pair every alias with its ref, and
//! tools take refs.
//!
//! **The size rule** is by rank width, not node count: a chat scales a
//! diagram to its column (about 748 px), so a graph three or four items wide
//! stays readable however tall it gets, and one six wide doesn't. See
//! [`layout`].

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

use factory_core::GraphState;
use factory_core::canonical::Indexed;
use factory_core::condition;
use factory_core::schema::{self, short_kind};
use factory_core::tool::Client;
use factory_proto::workgraph::v1 as wg;
use plan_tree::{Change, DiffEntry};
use serde_json::Value;

/// The widest rank drawn as is.
pub const MAX_WIDTH: usize = 4;
/// The widest rank drawn at all: left to right, or collapsed by phase.
pub const MAX_FALLBACK_WIDTH: usize = 6;
/// The most items drawn.
pub const MAX_NODES: usize = 40;
/// The longest title in a diagram's label.
pub const LABEL_CHARS: usize = 24;

/// The phases, in the order a collapsed graph lists them.
const PHASES: [&str; 5] = ["proposed", "approved", "done", "dropped", "unspecified"];

pub(crate) fn phase(node: &wg::Node) -> &'static str {
    match node.status.phase.as_known() {
        Some(wg::Phase::PHASE_PROPOSED) => "proposed",
        Some(wg::Phase::PHASE_APPROVED) => "approved",
        Some(wg::Phase::PHASE_DONE) => "done",
        Some(wg::Phase::PHASE_DROPPED) => "dropped",
        _ => "unspecified",
    }
}

/// The conditions that hold.
pub(crate) fn holds(node: &wg::Node) -> impl Iterator<Item = &str> {
    node.status
        .conditions
        .iter()
        .filter(|c| c.status)
        .map(|c| c.r#type.as_str())
}

// ---------------------------------------------------------------------------
// Text helpers

/// `title` cut to `max` characters, ending in `…` when cut.
pub fn cut(title: &str, max: usize) -> String {
    let title = one_line(title);
    if title.chars().count() <= max {
        return title;
    }
    let kept: String = title.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// Newlines and runs of whitespace as one space.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `text` for a quoted Mermaid label (`["…"]`): on one line, with the
/// characters Mermaid reads as syntax as entity codes.
pub fn mermaid_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in one_line(text).chars() {
        match c {
            '#' => out.push_str("#35;"),
            '"' => out.push_str("#quot;"),
            '[' => out.push_str("#91;"),
            ']' => out.push_str("#93;"),
            '{' => out.push_str("#123;"),
            '}' => out.push_str("#125;"),
            '(' => out.push_str("#40;"),
            ')' => out.push_str("#41;"),
            '|' => out.push_str("#124;"),
            '<' => out.push_str("#lt;"),
            '>' => out.push_str("#gt;"),
            '`' => out.push_str("#96;"),
            c => out.push(c),
        }
    }
    out
}

/// A diagram's label for an item: `T12: title`, the title cut to
/// [`LABEL_CHARS`], escaped.
pub fn label(alias: &str, title: &str) -> String {
    mermaid_text(&format!("{alias}: {}", cut(title, LABEL_CHARS)))
}

/// `text` for a GFM table cell: on one line, pipes escaped.
pub fn cell(text: &str) -> String {
    let text = one_line(text);
    if text.is_empty() {
        return "–".into();
    }
    text.replace('|', "\\|")
}

fn code(text: &str) -> String {
    format!("`{text}`")
}

// ---------------------------------------------------------------------------
// The outline: aliases in reading order

/// A graph with its items in outline order (depth first, children by ref)
/// and their aliases.
pub struct Outline<'g> {
    pub idx: Indexed<'g>,
    /// Every item but the root, in outline order.
    pub order: Vec<&'g wg::Node>,
    alias: HashMap<&'g str, String>,
}

impl<'g> Outline<'g> {
    pub fn new(graph: &'g wg::Graph) -> Self {
        let idx = Indexed::new(graph);
        let mut order: Vec<&wg::Node> = Vec::new();
        let mut seen: HashSet<&str> = HashSet::from([graph.root.as_str()]);
        fn walk<'g>(
            idx: &Indexed<'g>,
            at: &'g str,
            order: &mut Vec<&'g wg::Node>,
            seen: &mut HashSet<&'g str>,
        ) {
            for child in idx.children.get(at).into_iter().flatten() {
                if seen.insert(child)
                    && let Some(node) = idx.nodes.get(child)
                {
                    order.push(node);
                    walk(idx, child, order, seen);
                }
            }
        }
        walk(&idx, &graph.root, &mut order, &mut seen);
        // Anything off the hierarchy still gets a row.
        for node in &graph.nodes {
            if seen.insert(node.r#ref.as_str()) {
                order.push(node);
            }
        }
        let alias = order
            .iter()
            .enumerate()
            .map(|(i, n)| {
                let initial = short_kind(&n.kind)
                    .chars()
                    .next()
                    .map_or('N', |c| c.to_ascii_uppercase());
                (n.r#ref.as_str(), format!("{initial}{}", i + 1))
            })
            .collect();
        Self { idx, order, alias }
    }

    pub fn root(&self) -> Option<&'g wg::Node> {
        self.idx.nodes.get(self.idx.graph.root.as_str()).copied()
    }

    /// An item's alias; the root is `plan`, an unknown ref itself.
    pub fn alias<'a>(&'a self, r: &'a str) -> &'a str {
        if r == self.idx.graph.root {
            return "plan";
        }
        self.alias.get(r).map_or(r, String::as_str)
    }

    pub fn has(&self, r: &str) -> bool {
        self.idx.nodes.contains_key(r)
    }

    /// The items below `at`, in outline order.
    pub fn below(&self, at: &str) -> Vec<&'g wg::Node> {
        if at == self.idx.graph.root {
            return self.order.clone();
        }
        let mut set: HashSet<&str> = HashSet::new();
        let mut stack = vec![at];
        while let Some(r) = stack.pop() {
            for c in self.idx.children.get(r).into_iter().flatten() {
                if set.insert(c) {
                    stack.push(c);
                }
            }
        }
        self.order
            .iter()
            .filter(|n| set.contains(n.r#ref.as_str()))
            .copied()
            .collect()
    }

    /// What `r` waits for (`blocked_by` targets).
    pub fn blockers(&self, r: &str) -> &[&'g str] {
        self.idx
            .out
            .get(r)
            .and_then(|kinds| kinds.get(schema::BLOCKED_BY))
            .map_or(&[], Vec::as_slice)
    }

    /// The items of `refs` as `T1, T2`, or `–`.
    fn aliases<'a>(&self, refs: impl IntoIterator<Item = &'a str>) -> String {
        let list: Vec<&str> = refs.into_iter().map(|r| self.alias(r)).collect();
        if list.is_empty() {
            "–".into()
        } else {
            list.join(", ")
        }
    }
}

// ---------------------------------------------------------------------------
// The size rule

/// A layered graph's size: its widest rank and its number of ranks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shape {
    pub width: usize,
    pub depth: usize,
}

/// The shape of `n` nodes with `deps` edges (`(before, after)` indices) under
/// longest-path layering, as Mermaid's dagre ranks a flowchart: a node sits
/// one rank below the lowest of the nodes it follows, and every node that
/// follows none sits on the first. A node on a cycle stays on the first rank.
pub fn shape(n: usize, deps: &[(usize, usize)]) -> Shape {
    if n == 0 {
        return Shape { width: 0, depth: 0 };
    }
    let mut after: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indegree = vec![0usize; n];
    for &(a, b) in deps {
        if a < n && b < n && a != b {
            after[a].push(b);
            indegree[b] += 1;
        }
    }
    let mut rank = vec![0usize; n];
    let mut queue: Vec<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
    while let Some(i) = queue.pop() {
        for &j in &after[i] {
            rank[j] = rank[j].max(rank[i] + 1);
            indegree[j] -= 1;
            if indegree[j] == 0 {
                queue.push(j);
            }
        }
    }
    let depth = rank.iter().max().map_or(0, |m| m + 1);
    let mut count = vec![0usize; depth];
    for r in rank {
        count[r] += 1;
    }
    Shape {
        width: count.into_iter().max().unwrap_or(0),
        depth,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    TopDown,
    LeftRight,
}

/// Why a graph was left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Omitted {
    Empty,
    TooMany { nodes: usize },
    TooWide { width: usize },
}

/// How a graph is drawn, if at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Every item.
    Graph(Direction),
    /// One node per phase: too wide to draw as is, too deep to turn.
    Collapsed { width: usize, depth: usize },
    /// The table only.
    TableOnly(Omitted),
}

/// The size rule. At most [`MAX_WIDTH`] wide, top down. Up to
/// [`MAX_FALLBACK_WIDTH`], left to right if it's at most [`MAX_WIDTH`] deep,
/// else collapsed by phase. Wider than that, or more than [`MAX_NODES`]
/// items, the table only.
pub fn layout(nodes: usize, shape: Shape) -> Layout {
    if nodes == 0 {
        return Layout::TableOnly(Omitted::Empty);
    }
    if nodes > MAX_NODES {
        return Layout::TableOnly(Omitted::TooMany { nodes });
    }
    if shape.width <= MAX_WIDTH {
        return Layout::Graph(Direction::TopDown);
    }
    if shape.width <= MAX_FALLBACK_WIDTH {
        if shape.depth <= MAX_WIDTH {
            return Layout::Graph(Direction::LeftRight);
        }
        return Layout::Collapsed {
            width: shape.width,
            depth: shape.depth,
        };
    }
    Layout::TableOnly(Omitted::TooWide { width: shape.width })
}

// ---------------------------------------------------------------------------
// The graph

/// The class an item is drawn with: its phase, or a condition that matters
/// more while it's open.
fn class(node: &wg::Node) -> &'static str {
    let p = phase(node);
    if matches!(p, "done" | "dropped") {
        return p;
    }
    let has = |c: &str| holds(node).any(|h| h == c);
    if has(condition::STALLED) {
        "stalled"
    } else if has(condition::RUNNING) || has(condition::IN_REVIEW) || has(condition::AWAITING_GATE)
    {
        "running"
    } else if has(condition::BLOCKED) {
        "blocked"
    } else if has(condition::READY) {
        "ready"
    } else {
        p
    }
}

fn class_def(name: &str) -> &'static str {
    match name {
        "proposed" => "fill:#f5f5f5,stroke:#9e9e9e,stroke-dasharray:4 3",
        "approved" => "fill:#e3f2fd,stroke:#1e88e5",
        "done" => "fill:#e8f5e9,stroke:#43a047",
        "dropped" => "fill:#fafafa,stroke:#bdbdbd,color:#9e9e9e",
        "ready" => "fill:#fff8e1,stroke:#f9a825",
        "blocked" => "fill:#fbe9e7,stroke:#e64a19",
        "running" => "fill:#ede7f6,stroke:#5e35b1",
        "stalled" => "fill:#ffebee,stroke:#c62828,stroke-width:2px",
        _ => "fill:#ffffff,stroke:#9e9e9e",
    }
}

/// What one diagram draws: the items in scope, which of them are
/// containers (drawn as subgraphs), and the edges among them.
struct Scene<'o, 'g> {
    outline: &'o Outline<'g>,
    /// Where the scope starts (the root, or `under`).
    top: &'g str,
    items: Vec<&'g wg::Node>,
    containers: HashSet<&'g str>,
    /// The items drawn as nodes, by index.
    leaves: Vec<&'g str>,
    /// `(before, after)` leaf indices, for ranking.
    deps: Vec<(usize, usize)>,
}

impl<'o, 'g> Scene<'o, 'g> {
    fn new(outline: &'o Outline<'g>, top: &'g str) -> Self {
        let items = outline.below(top);
        let in_scope: HashSet<&str> = items.iter().map(|n| n.r#ref.as_str()).collect();
        let containers: HashSet<&str> = items
            .iter()
            .map(|n| n.r#ref.as_str())
            .filter(|r| {
                outline
                    .idx
                    .children
                    .get(r)
                    .is_some_and(|c| c.iter().any(|c| in_scope.contains(c)))
            })
            .collect();
        let leaves: Vec<&str> = items
            .iter()
            .map(|n| n.r#ref.as_str())
            .filter(|r| !containers.contains(r))
            .collect();
        let index: HashMap<&str, usize> = leaves.iter().enumerate().map(|(i, r)| (*r, i)).collect();
        // A container stands for every leaf below it.
        let expand = |r: &'g str| -> Vec<usize> {
            if let Some(&i) = index.get(r) {
                return vec![i];
            }
            outline
                .below(r)
                .iter()
                .filter_map(|n| index.get(n.r#ref.as_str()).copied())
                .collect()
        };
        let mut deps = Vec::new();
        for waiter in &items {
            for blocker in outline.blockers(&waiter.r#ref) {
                if !in_scope.contains(blocker) {
                    continue;
                }
                for b in expand(blocker) {
                    for a in expand(&waiter.r#ref) {
                        if a != b {
                            deps.push((b, a));
                        }
                    }
                }
            }
        }
        Self {
            outline,
            top,
            items,
            containers,
            leaves,
            deps,
        }
    }

    fn in_scope(&self, r: &str) -> bool {
        self.items.iter().any(|n| n.r#ref == r)
    }

    fn layout(&self) -> Layout {
        layout(self.items.len(), shape(self.leaves.len(), &self.deps))
    }

    fn mermaid(&self, direction: Direction) -> String {
        let mut out = String::from("```mermaid\n");
        out.push_str(match direction {
            Direction::TopDown => "flowchart TD\n",
            Direction::LeftRight => "flowchart LR\n",
        });
        let mut used: BTreeSet<&str> = BTreeSet::new();
        for item in &self.items {
            used.insert(class(item));
        }
        for name in PHASES
            .iter()
            .chain(&["ready", "blocked", "running", "stalled"])
        {
            if used.contains(name) {
                let _ = writeln!(out, "  classDef {name} {}", class_def(name));
            }
        }
        self.nodes(self.top, 1, &mut out);
        let o = self.outline;
        for waiter in &self.items {
            for blocker in o.blockers(&waiter.r#ref) {
                if self.in_scope(blocker) && !self.nested(blocker, &waiter.r#ref) {
                    let _ = writeln!(out, "  {} --> {}", o.alias(blocker), o.alias(&waiter.r#ref));
                }
            }
        }
        for e in &o.idx.graph.edges {
            if e.kind == schema::CHILD_OF || e.kind == schema::BLOCKED_BY {
                continue;
            }
            if self.in_scope(&e.from) && self.in_scope(&e.to) && !self.nested(&e.from, &e.to) {
                let _ = writeln!(
                    out,
                    "  {} -. {} .-> {}",
                    o.alias(&e.from),
                    mermaid_text(&e.kind),
                    o.alias(&e.to)
                );
            }
        }
        // A subgraph takes its class after it's defined.
        for item in &self.items {
            if self.containers.contains(item.r#ref.as_str()) {
                let _ = writeln!(out, "  class {} {}", o.alias(&item.r#ref), class(item));
            }
        }
        out.push_str("```\n");
        out
    }

    /// Whether one of `a` and `b` contains the other. Mermaid's layout
    /// can't draw an edge between a subgraph and what's inside it, so such
    /// edges are only in the table.
    fn nested(&self, a: &str, b: &str) -> bool {
        let parent = &self.outline.idx.parent;
        let above = |start: &str, other: &str| {
            let mut at = parent.get(start).copied();
            while let Some(p) = at {
                if p == other {
                    return true;
                }
                at = parent.get(p).copied();
            }
            false
        };
        above(a, b) || above(b, a)
    }

    fn nodes(&self, at: &str, depth: usize, out: &mut String) {
        let o = self.outline;
        let pad = "  ".repeat(depth);
        for child in o.idx.children.get(at).into_iter().flatten() {
            if !self.in_scope(child) {
                continue;
            }
            let Some(n) = o.idx.nodes.get(child) else {
                continue;
            };
            let alias = o.alias(child);
            if self.containers.contains(child) {
                let _ = writeln!(
                    out,
                    "{pad}subgraph {alias}[\"{}\"]",
                    label(alias, &n.spec.title)
                );
                self.nodes(child, depth + 1, out);
                let _ = writeln!(out, "{pad}end");
            } else {
                let _ = writeln!(
                    out,
                    "{pad}{alias}[\"{}\"]:::{}",
                    label(alias, &n.spec.title),
                    class(n)
                );
            }
        }
    }

    fn collapsed(&self) -> String {
        let o = self.outline;
        let mut count: HashMap<&str, usize> = HashMap::new();
        let phase_of: Vec<&str> = self
            .leaves
            .iter()
            .map(|r| o.idx.nodes.get(r).map_or("unspecified", |n| phase(n)))
            .collect();
        for p in &phase_of {
            *count.entry(p).or_default() += 1;
        }
        let mut out = String::from("```mermaid\nflowchart LR\n");
        for p in PHASES {
            if count.contains_key(p) {
                let _ = writeln!(out, "  classDef {p} {}", class_def(p));
            }
        }
        for p in PHASES {
            if let Some(n) = count.get(p) {
                let items = if *n == 1 { "item" } else { "items" };
                let _ = writeln!(out, "  phase_{p}[\"{p}: {n} {items}\"]:::{p}");
            }
        }
        let mut links: BTreeSet<(usize, usize)> = BTreeSet::new();
        let pos = |p: &str| PHASES.iter().position(|q| *q == p).unwrap_or(PHASES.len());
        for &(a, b) in &self.deps {
            let (pa, pb) = (phase_of[a], phase_of[b]);
            if pa != pb {
                links.insert((pos(pa), pos(pb)));
            }
        }
        for (a, b) in links {
            let _ = writeln!(out, "  phase_{} --> phase_{}", PHASES[a], PHASES[b]);
        }
        out.push_str("```\n");
        out
    }
}

// ---------------------------------------------------------------------------
// Results

fn link(client: &Client, root: &str) -> Option<String> {
    let base = client.control_room.as_deref()?.trim_end_matches('/');
    let sep = if base.contains('?') { '&' } else { '?' };
    let slash = if base.contains('?') { "" } else { "/" };
    Some(format!("{base}{slash}{sep}plan={}", encode(root)))
}

/// Percent-encodes all but RFC 3986's unreserved characters.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// The plan's header: title, goal, rubric, budget, revisions and drift.
fn header(state: &GraphState, o: &Outline<'_>, out: &mut String) {
    let root = &state.graph.root;
    let node = o.root();
    let title = node.map_or("", |n| n.spec.title.as_str());
    let _ = writeln!(out, "## Plan: {}", one_line(title));
    let _ = writeln!(
        out,
        "ref {} · {}",
        code(root),
        node.map_or("unspecified", phase)
    );
    if let Some(plan) = node
        .and_then(|n| n.spec.payload.as_option())
        .and_then(|a| a.unpack_unchecked::<wg::Plan>().ok())
    {
        if !plan.goal.is_empty() {
            let _ = writeln!(out, "\n**Goal:** {}", one_line(&plan.goal));
        }
        if !plan.rubric.is_empty() {
            out.push_str("\n**Rubric:**\n");
            for r in &plan.rubric {
                let _ = writeln!(out, "- {}", one_line(r));
            }
        }
        if plan.budget_usd > 0.0 {
            let _ = writeln!(out, "\n**Budget:** ${:.2}", plan.budget_usd);
        }
    }
    out.push('\n');
    revisions(state, o, out);
}

fn revisions(state: &GraphState, o: &Outline<'_>, out: &mut String) {
    let _ = write!(out, "**Revision** {}", code(&state.revision));
    match state.approved.as_deref() {
        None => out.push_str(" · never approved\n"),
        Some(a) if a == state.revision => out.push_str(" · approved, unchanged since\n"),
        Some(a) => {
            let _ = write!(out, " · **approved** {}", code(a));
            if state.drifted.is_empty() {
                out.push('\n');
            } else {
                let _ = writeln!(
                    out,
                    " · **drifted:** {} ({} since the approved revision)",
                    o.aliases(state.drifted.iter().map(String::as_str)),
                    plural(state.drifted.len(), "item")
                );
            }
        }
    }
}

fn plural(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// `plan_read`'s text: the header, the graph (or why it's left out), a row
/// per item, and the other edges. `under` narrows it to an item's subtree;
/// it must be in the graph.
pub fn plan(state: &GraphState, under: Option<&str>, client: &Client) -> String {
    let o = Outline::new(&state.graph);
    let root = state.graph.root.as_str();
    let top = under
        .filter(|u| *u != root)
        .and_then(|u| o.idx.nodes.get(u))
        .map_or(root, |n| n.r#ref.as_str());
    let mut out = String::new();
    if top == root {
        header(state, &o, &mut out);
    } else {
        let n = o.idx.nodes[top];
        let plan_title = o.root().map_or("", |r| r.spec.title.as_str());
        let _ = writeln!(
            out,
            "## {}: {} (part of the plan {})",
            o.alias(top),
            one_line(&n.spec.title),
            one_line(plan_title)
        );
        let conditions: Vec<&str> = holds(n).collect();
        let _ = writeln!(
            out,
            "ref {} · {} {} · {}{}\n",
            code(top),
            short_kind(&n.kind),
            phase(n),
            if conditions.is_empty() {
                "no conditions".to_owned()
            } else {
                conditions.join(", ")
            },
            o.blockers(top)
                .iter()
                .map(|b| format!(" · blocked by {}", o.alias(b)))
                .collect::<String>()
        );
        revisions(state, &o, &mut out);
    }
    out.push('\n');

    let scene = Scene::new(&o, top);
    let n = scene.items.len();
    let more = link(client, root).map_or_else(
        || "the full graph is in the control room".to_owned(),
        |l| format!("the full graph is in the control room: {l}"),
    );
    let narrow = "Read a part with `plan_read` and `under: <ref>`";
    match scene.layout() {
        Layout::Graph(d) => out.push_str(&scene.mermaid(d)),
        Layout::Collapsed { width, depth } => {
            let _ = writeln!(
                out,
                "Graph collapsed to one node per phase: its widest rank has {width} items \
                 (more than {MAX_WIDTH}), and it's {depth} ranks deep, too deep to turn. \
                 {narrow}; {more}.\n"
            );
            out.push_str(&scene.collapsed());
        }
        Layout::TableOnly(Omitted::Empty) => {
            out.push_str("No items yet.\n");
            return out;
        }
        Layout::TableOnly(Omitted::TooMany { nodes }) => {
            let _ = writeln!(
                out,
                "Graph left out: {nodes} items, more than {MAX_NODES}. {narrow}; {more}."
            );
        }
        Layout::TableOnly(Omitted::TooWide { width }) => {
            let _ = writeln!(
                out,
                "Graph left out: its widest rank has {width} items, more than \
                 {MAX_FALLBACK_WIDTH}. {narrow}; {more}."
            );
        }
    }
    out.push('\n');

    let nested = scene.items.iter().any(|i| {
        o.idx
            .parent
            .get(i.r#ref.as_str())
            .is_some_and(|p| *p != top)
    });
    let _ = writeln!(out, "**Items** ({n})\n");
    out.push_str("| # | ref | kind | title | phase | conditions | blocked_by |");
    out.push_str(if nested { " parent |\n" } else { "\n" });
    out.push_str("|---|---|---|---|---|---|---|");
    out.push_str(if nested { "---|\n" } else { "\n" });
    for item in &scene.items {
        let r = item.r#ref.as_str();
        let conditions: Vec<&str> = holds(item).collect();
        let _ = write!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} |",
            o.alias(r),
            code(r),
            short_kind(&item.kind),
            cell(&item.spec.title),
            phase(item),
            cell(&conditions.join(", ")),
            o.aliases(o.blockers(r).iter().copied())
        );
        if nested {
            let _ = write!(
                out,
                " {} |",
                o.idx.parent.get(r).map_or("–", |p| o.alias(p))
            );
        }
        out.push('\n');
    }

    let other: Vec<&wg::Edge> = state
        .graph
        .edges
        .iter()
        .filter(|e| e.kind != schema::CHILD_OF && e.kind != schema::BLOCKED_BY)
        .filter(|e| top == root || scene.in_scope(&e.from) || scene.in_scope(&e.to))
        .collect();
    if !other.is_empty() {
        let _ = writeln!(out, "\n**Other edges** ({})\n", other.len());
        for e in other {
            let _ = writeln!(
                out,
                "- {} {} {}",
                o.alias(&e.from),
                code(&e.kind),
                o.alias(&e.to)
            );
        }
    }
    out
}

/// `ready`'s text: what can start now.
pub fn ready(state: &GraphState) -> String {
    let o = Outline::new(&state.graph);
    let title = o.root().map_or("", |r| r.spec.title.as_str());
    let ready: Vec<&wg::Node> = o
        .order
        .iter()
        .filter(|n| holds(n).any(|c| c == condition::READY))
        .copied()
        .collect();
    let mut out = String::new();
    let _ = writeln!(
        out,
        "## Ready: {} in the plan {}\n",
        plural(ready.len(), "item"),
        one_line(title)
    );
    if ready.is_empty() {
        out.push_str(
            "Nothing can start now. `plan_read` shows what's blocked, running or waiting for \
             approval.\n",
        );
        return out;
    }
    out.push_str("| # | ref | kind | title |\n|---|---|---|---|\n");
    for n in ready {
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} |",
            o.alias(&n.r#ref),
            code(&n.r#ref),
            short_kind(&n.kind),
            cell(&n.spec.title)
        );
    }
    out
}

/// The changes from the approved revision to the current one (a compact
/// diff), as a table.
pub fn changes(state: &GraphState, entries: &[DiffEntry]) -> String {
    let o = Outline::new(&state.graph);
    let mut out = String::new();
    let rows: Vec<&DiffEntry> = entries
        .iter()
        .filter(|e| e.change != Change::ModifiedDescendants)
        .collect();
    match state.approved.as_deref() {
        None => {
            let _ = writeln!(
                out,
                "**Changes:** never approved, so every item is new ({}).",
                plural(rows.len(), "item")
            );
            if rows.is_empty() {
                return out;
            }
        }
        Some(_) if rows.is_empty() => {
            out.push_str("**Changes:** none since the approved revision.\n");
            return out;
        }
        Some(a) => {
            let _ = writeln!(
                out,
                "**Changes since the approved revision** {} ({}):",
                code(a),
                plural(rows.len(), "item")
            );
        }
    }
    out.push_str("\n| change | # | ref | kind | title |\n|---|---|---|---|---|\n");
    for e in rows {
        let change = match e.change {
            Change::Added => "added".to_owned(),
            Change::Removed => "removed".to_owned(),
            Change::Modified | Change::ModifiedDescendants => "modified".to_owned(),
            Change::Moved => {
                let from = e.from_parent.as_deref().map_or("?", |p| o.alias(p));
                let to = e.to_parent.as_deref().map_or("?", |p| o.alias(p));
                let edited = if e.content_changed {
                    " and modified"
                } else {
                    ""
                };
                format!("moved from {from} to {to}{edited}")
            }
        };
        let alias = if o.has(&e.key) {
            o.alias(&e.key)
        } else {
            "–"
        };
        let _ = writeln!(
            out,
            "| {change} | {alias} | {} | {} | {} |",
            code(&e.key),
            short_kind(&e.kind),
            cell(&e.label)
        );
    }
    out
}

/// Where a person approves the plan, for a client that can't show plan
/// review: the control room's link, or where to go without one. Empty for
/// an MCP Apps client.
pub fn approve_where(client: &Client, root: &str, revision: &str, title: &str) -> String {
    if client.apps {
        return String::new();
    }
    match link(client, root) {
        Some(l) => format!(
            "**Approve it in the control room:** {l}\n\nA person approves revision {} \
             there; you can't. Until they do, don't treat the plan as approved.\n",
            code(revision)
        ),
        None => format!(
            "A person approves revision {} in the factory's control room (the plan {}, {}); \
             you can't, and no link to it is configured here. Until they do, don't treat the \
             plan as approved.\n",
            code(revision),
            one_line(title),
            code(root)
        ),
    }
}

/// `submit`'s text.
pub fn submitted(
    state: &GraphState,
    entries: &[DiffEntry],
    attention: Option<&str>,
    client: &Client,
) -> String {
    let o = Outline::new(&state.graph);
    let title = o.root().map_or("", |r| r.spec.title.as_str());
    let mut out = String::new();
    let _ = writeln!(
        out,
        "## Submitted: {}\n\nRevision {} of {} is awaiting approval.{}\n",
        one_line(title),
        code(&state.revision),
        code(&state.graph.root),
        attention.map_or_else(String::new, |a| format!(
            " The approval request is {}.",
            code(a)
        ))
    );
    out.push_str(&changes(state, entries));
    out.push('\n');
    if client.apps {
        out.push_str(
            "The person approves it, or returns it, in plan review (`open_plan_review`).\n",
        );
    } else {
        out.push_str(&approve_where(
            client,
            &state.graph.root,
            &state.revision,
            title,
        ));
    }
    out
}

/// `open_plan_review`'s text. An MCP Apps client shows the review; any
/// other gets the control room's link instead.
pub fn review(state: &GraphState, entries: &[DiffEntry], client: &Client) -> String {
    let root = state.graph.root.as_str();
    let revision = state.revision.as_str();
    let o = Outline::new(&state.graph);
    let title = o.root().map_or("", |r| r.spec.title.as_str());
    let since = match state.approved.as_deref() {
        Some(a) if a == revision => "unchanged since its approval".to_owned(),
        Some(a) => format!("compared with the approved revision {a}"),
        None => "never approved before".to_owned(),
    };
    let mut out = if client.apps {
        format!(
            "Opened the review of the plan {title:?} ({root}) at revision {revision}, {since}. \
             Nothing is approved until the person decides; the outcome arrives as an \
             `<mcp-app-context>` message.\n"
        )
    } else {
        format!(
            "Plan review can't be shown in this client. The plan {title:?} ({root}) is at \
             revision {revision}, {since}. Nothing is approved until a person decides in the \
             control room.\n"
        )
    };
    out.push('\n');
    out.push_str(&changes(state, entries));
    if !client.apps {
        out.push('\n');
        out.push_str(&approve_where(client, root, revision, title));
    }
    out
}

/// `propose`'s text.
pub fn proposed(
    node: &str,
    attention: &str,
    revision: &str,
    root: &str,
    client: &Client,
) -> String {
    let mut out = format!(
        "Proposed as {} against revision {}. A person accepts or declines it; `item_read` on \
         {} shows the outcome under `attention`.",
        code(attention),
        code(revision),
        code(node)
    );
    if !client.apps {
        match link(client, root) {
            Some(l) => {
                let _ = write!(out, "\n\n**Decide it in the control room:** {l}");
            }
            None => out.push_str(" A person decides it in the factory's control room."),
        }
    }
    out.push('\n');
    out
}

// ---------------------------------------------------------------------------
// One item

/// A proto-JSON payload's fields as a list, its `@type` left out. Strings
/// are markdown, so they stay as they are; lists of strings are code.
fn fields(value: &Value, out: &mut String) {
    let Value::Object(map) = value else {
        let _ = writeln!(out, "- {}", code(&value.to_string()));
        return;
    };
    for (key, v) in map {
        if key == "@type" {
            continue;
        }
        match v {
            Value::String(s) if s.contains('\n') => {
                let _ = writeln!(out, "- **{key}:**");
                for line in s.lines() {
                    let _ = writeln!(out, "  {line}");
                }
            }
            Value::String(s) => {
                let _ = writeln!(out, "- **{key}:** {s}");
            }
            Value::Array(items) if items.iter().all(Value::is_string) => {
                let list: Vec<String> = items.iter().filter_map(Value::as_str).map(code).collect();
                let _ = writeln!(out, "- **{key}:** {}", list.join(", "));
            }
            other => {
                let _ = writeln!(out, "- **{key}:** {}", code(&other.to_string()));
            }
        }
    }
}

/// A payload as proto-JSON: the core's kinds decoded here, anything else
/// through the JSON registry. `None` when neither can read it.
fn any_json(any: &buffa_types::google::protobuf::Any) -> Option<Value> {
    fn as_json<M>(any: &buffa_types::google::protobuf::Any) -> Option<Value>
    where
        M: buffa::Message + Default + serde::Serialize,
    {
        serde_json::to_value(any.unpack_unchecked::<M>().ok()?).ok()
    }
    let known = match any.type_url.as_str() {
        schema::PLAN => as_json::<wg::Plan>(any),
        schema::TASK => as_json::<wg::Task>(any),
        schema::DECISION => as_json::<wg::Decision>(any),
        schema::GATE => as_json::<wg::Gate>(any),
        schema::LEARNING => as_json::<wg::Learning>(any),
        schema::RECONCILE_NOTE => as_json::<wg::ReconcileNote>(any),
        schema::DECISION_OUTCOME => as_json::<wg::DecisionOutcome>(any),
        wg::ReviewVerdict::TYPE_URL => as_json::<wg::ReviewVerdict>(any),
        _ => None,
    };
    let value = known.or_else(|| serde_json::to_value(any).ok())?;
    // An `Any` the registry doesn't know serializes as its bytes.
    match &value {
        Value::Object(m) if m.contains_key("value") && m.len() <= 2 && m.contains_key("@type") => {
            None
        }
        _ => Some(value),
    }
}

fn run_state(run: &wg::Run) -> &'static str {
    match run.state.as_known() {
        Some(wg::RunState::RUN_STATE_QUEUED) => "queued",
        Some(wg::RunState::RUN_STATE_WORKING) => "working",
        Some(wg::RunState::RUN_STATE_INPUT_REQUIRED) => "input required",
        Some(wg::RunState::RUN_STATE_COMPLETED) => "completed",
        Some(wg::RunState::RUN_STATE_FAILED) => "failed",
        Some(wg::RunState::RUN_STATE_CANCELED) => "canceled",
        _ => "unspecified",
    }
}

fn attention_kind(a: &wg::Attention) -> &'static str {
    use wg::AttentionKind as K;
    match a.kind.as_known() {
        Some(K::ATTENTION_KIND_ASK) => "ask",
        Some(K::ATTENTION_KIND_APPROVAL) => "approval",
        Some(K::ATTENTION_KIND_GATE) => "gate",
        Some(K::ATTENTION_KIND_STALLED) => "stalled",
        Some(K::ATTENTION_KIND_REVIEW_ESCALATION) => "review escalation",
        Some(K::ATTENTION_KIND_BUDGET) => "budget",
        Some(K::ATTENTION_KIND_PROPOSAL) => "proposal",
        _ => "unspecified",
    }
}

fn artifact(a: &wg::Artifact) -> Option<String> {
    match a.artifact.as_ref()? {
        wg::artifact::Artifact::PullRequest(pr) => {
            let name = if pr.number > 0 {
                format!("{}#{}", pr.repo, pr.number)
            } else {
                pr.url.clone()
            };
            let state = if pr.state.is_empty() {
                String::new()
            } else {
                format!(" ({})", pr.state)
            };
            Some(format!("[{name}]({}){state}", pr.url))
        }
        wg::artifact::Artifact::Release(r) => Some(format!("{} {}", r.package, r.version)),
    }
}

/// What an item's annotations and runs say, beyond the graph.
pub struct ItemRecords<'a> {
    pub annotations: &'a [wg::Annotation],
    pub runs: &'a [wg::Run],
    pub attention: &'a [wg::Attention],
}

/// `item_read`'s text: the item, its payload, its edges, annotations, runs
/// and attention items.
pub fn item(state: &GraphState, node: &wg::Node, records: &ItemRecords<'_>) -> String {
    let o = Outline::new(&state.graph);
    let r = node.r#ref.as_str();
    let mut out = String::new();
    let alias = o.alias(r);
    let _ = writeln!(
        out,
        "## {} {alias}: {}",
        short_kind(&node.kind),
        one_line(&node.spec.title)
    );
    let conditions: Vec<&str> = holds(node).collect();
    let parent = o
        .idx
        .parent
        .get(r)
        .map(|p| format!(" · parent {} {}", o.alias(p), code(p)));
    let _ = writeln!(
        out,
        "ref {}{} · {} · {}\n",
        code(r),
        parent.unwrap_or_default(),
        phase(node),
        if conditions.is_empty() {
            "no conditions".to_owned()
        } else {
            conditions.join(", ")
        }
    );
    for c in node
        .status
        .conditions
        .iter()
        .filter(|c| c.status && !c.message.is_empty())
    {
        let _ = writeln!(out, "- {}: {}", c.r#type, one_line(&c.message));
    }
    if node
        .status
        .conditions
        .iter()
        .any(|c| c.status && !c.message.is_empty())
    {
        out.push('\n');
    }
    if !node.spec.body.trim().is_empty() {
        let _ = writeln!(out, "{}\n", node.spec.body.trim());
    }
    if let Some(any) = node.spec.payload.as_option() {
        let _ = writeln!(out, "**{}**\n", short_kind(&node.kind));
        match any_json(any) {
            Some(v) => fields(&v, &mut out),
            None => {
                let _ = writeln!(out, "- a {} payload", code(&any.type_url));
            }
        }
        out.push('\n');
    }
    let labels = &node.metadata.labels;
    if !labels.is_empty() {
        let mut kv: Vec<String> = labels
            .iter()
            .map(|(k, v)| code(&format!("{k}={v}")))
            .collect();
        kv.sort();
        let _ = writeln!(out, "**Labels:** {}\n", kv.join(", "));
    }

    let mut edges = Vec::new();
    for e in &state.graph.edges {
        if e.kind == schema::CHILD_OF {
            continue;
        }
        if e.from == r {
            edges.push(format!(
                "{} {} {}",
                code(&e.kind),
                o.alias(&e.to),
                code(&e.to)
            ));
        } else if e.to == r {
            edges.push(format!(
                "{} {} {} this item",
                o.alias(&e.from),
                code(&e.from),
                code(&e.kind)
            ));
        }
    }
    let children: Vec<&str> = o
        .idx
        .children
        .get(r)
        .map(|c| c.to_vec())
        .unwrap_or_default();
    if !children.is_empty() {
        edges.push(format!("children {}", o.aliases(children)));
    }
    if !edges.is_empty() {
        out.push_str("**Edges**\n\n");
        for e in edges {
            let _ = writeln!(out, "- {e}");
        }
        out.push('\n');
    }

    if !records.annotations.is_empty() {
        let _ = writeln!(out, "**Annotations** ({})\n", records.annotations.len());
        for a in records.annotations {
            let by = a
                .provenance
                .as_option()
                .map(|p| format!(" by {}", p.actor))
                .unwrap_or_default();
            let _ = writeln!(out, "- {} {}{by}", short_kind(&a.kind), code(&a.r#ref));
            if let Some(v) = a.payload.as_option().and_then(any_json) {
                let mut inner = String::new();
                fields(&v, &mut inner);
                for line in inner.lines() {
                    let _ = writeln!(out, "  {line}");
                }
            }
        }
        out.push('\n');
    }

    if !records.runs.is_empty() {
        let _ = writeln!(out, "**Runs** ({})\n", records.runs.len());
        out.push_str("| run | attempt | executor | state | stage | cost | artifacts |\n");
        out.push_str("|---|---|---|---|---|---|---|\n");
        for run in records.runs {
            let artifacts: Vec<String> = run.artifacts.iter().filter_map(artifact).collect();
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} | {} |",
                code(&run.r#ref),
                run.attempt,
                cell(&run.executor),
                run_state(run),
                cell(&run.stage),
                if run.cost_usd > 0.0 {
                    format!("${:.2}", run.cost_usd)
                } else {
                    "–".into()
                },
                cell(&artifacts.join(", "))
            );
        }
        for run in records.runs.iter().filter(|r| !r.error.is_empty()) {
            let _ = writeln!(
                out,
                "\n{} failed: {}",
                code(&run.r#ref),
                one_line(&run.error)
            );
        }
        out.push('\n');
    }

    if !records.attention.is_empty() {
        let _ = writeln!(out, "**Attention** ({})\n", records.attention.len());
        for a in records.attention {
            let state = if a.resolved {
                let by = if a.resolved_by.is_empty() {
                    String::new()
                } else {
                    format!(" by {}", a.resolved_by)
                };
                let comment = if a.comment.is_empty() {
                    String::new()
                } else {
                    format!(": {}", one_line(&a.comment))
                };
                format!(
                    "{}{by}{comment}",
                    if a.resolution.is_empty() {
                        "resolved"
                    } else {
                        &a.resolution
                    }
                )
            } else {
                "open".into()
            };
            let _ = writeln!(
                out,
                "- {} {} ({state}): {}",
                attention_kind(a),
                code(&a.r#ref),
                one_line(&a.summary)
            );
        }
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use factory_core::payload;
    use factory_core::schema::{DECISION, GATE, PLAN, TASK};

    use super::*;

    fn r(i: usize) -> String {
        format!("olai:019ab3c2-5b1e-7f00-8000-{i:012}")
    }

    const REV: &str = "4be1d0c39a7f25e8b1c6d4f0a9e37b2c5d8f1a6e0b3c7d9e2f4a1b5c8d0e3f6a";
    const APPROVED: &str = "a17c9e2d4f6b8a0c3e5d7f9b1a3c5e7d9f0b2a4c6e8d0f1a3b5c7e9d2f4a6b8c";

    fn node(i: usize, kind: &str, title: &str, phase: wg::Phase, conditions: &[&str]) -> wg::Node {
        wg::Node {
            r#ref: r(i),
            kind: kind.into(),
            spec: wg::NodeSpec {
                title: title.into(),
                ..Default::default()
            }
            .into(),
            status: wg::NodeStatus {
                phase: phase.into(),
                conditions: conditions
                    .iter()
                    .map(|c| wg::Condition {
                        r#type: (*c).into(),
                        status: true,
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }
    }

    fn edge(kind: &str, from: usize, to: usize) -> wg::Edge {
        wg::Edge {
            kind: kind.into(),
            from: r(from),
            to: r(to),
            ..Default::default()
        }
    }

    fn plan_node(title: &str, budget: f64) -> wg::Node {
        let mut n = node(0, PLAN, title, wg::Phase::PHASE_APPROVED, &[]);
        n.spec = payload::spec(
            title,
            "",
            &wg::Plan {
                goal: "An approved plan runs end to end on olai-store, with no GitHub issues."
                    .into(),
                rubric: vec![
                    "The conformance suite passes on every backend.".into(),
                    "Every revision names its actor.".into(),
                ],
                budget_usd: budget,
                ..Default::default()
            },
        )
        .into();
        n
    }

    /// A graph of `nodes` (the root first), each a child of the root unless
    /// `parents` says otherwise, plus `edges`.
    fn state(
        nodes: Vec<wg::Node>,
        parents: &[(usize, usize)],
        edges: Vec<wg::Edge>,
        approved: Option<&str>,
        drifted: &[usize],
    ) -> GraphState {
        let mut all = Vec::new();
        for n in nodes.iter().skip(1) {
            let i: usize = n.r#ref.rsplit('-').next().unwrap().parse().unwrap();
            let p = parents.iter().find(|(c, _)| *c == i).map_or(0, |(_, p)| *p);
            all.push(edge(schema::CHILD_OF, i, p));
        }
        all.extend(edges);
        GraphState {
            graph: wg::Graph {
                root: r(0),
                nodes,
                edges: all,
                ..Default::default()
            },
            revision: REV.into(),
            approved: approved.map(Into::into),
            drifted: drifted.iter().map(|i| r(*i)).collect(),
        }
    }

    use wg::Phase::{PHASE_APPROVED as APP, PHASE_DONE as DONE, PHASE_PROPOSED as PROP};

    /// The demo's shape: a chain of three, a task, a decision and a gate.
    fn small() -> GraphState {
        state(
            vec![
                plan_node("Typed work graph on olai-store", 25.0),
                node(1, TASK, "Define factory.workgraph.v1", DONE, &[]),
                node(2, TASK, "Build the core on olai-store", APP, &["Ready"]),
                node(3, TASK, "Serve factory.v1 over Connect", APP, &["Blocked"]),
                node(4, TASK, "Control-room UI", APP, &["Running"]),
                node(5, DECISION, "Where do revisions live?", PROP, &["Blocked"]),
                node(
                    6,
                    GATE,
                    "Wait for the \"core\" PR [#12] | merged",
                    APP,
                    &["Blocked"],
                ),
            ],
            &[],
            vec![
                edge(schema::BLOCKED_BY, 2, 1),
                edge(schema::BLOCKED_BY, 3, 2),
                edge(schema::BLOCKED_BY, 6, 2),
                edge(schema::RELATES_TO, 4, 5),
            ],
            Some(APPROVED),
            &[5],
        )
    }

    /// Seven tasks side by side after one: too wide to draw as is, shallow
    /// enough to turn.
    fn wide() -> GraphState {
        let mut nodes = vec![
            plan_node("Fan out the connectors", 0.0),
            node(1, TASK, "Shared connector trait", DONE, &[]),
        ];
        let names = [
            "Postgres connector",
            "MySQL connector",
            "SQLite connector",
            "DuckDB connector",
            "Snowflake connector",
            "BigQuery connector",
        ];
        let mut edges = Vec::new();
        for (i, n) in names.iter().enumerate() {
            nodes.push(node(i + 2, TASK, n, APP, &["Ready"]));
            edges.push(edge(schema::BLOCKED_BY, i + 2, 1));
        }
        nodes.push(node(
            8,
            TASK,
            "Connector conformance suite",
            PROP,
            &["Blocked"],
        ));
        for i in 2..8 {
            edges.push(edge(schema::BLOCKED_BY, 8, i));
        }
        state(nodes, &[], edges, None, &[])
    }

    /// Too many items: three epics of fifteen tasks each.
    fn large() -> GraphState {
        let mut nodes = vec![plan_node("Migrate every service to olai-store", 120.0)];
        let mut parents = Vec::new();
        let mut edges = Vec::new();
        let mut i = 1;
        for (e, epic) in ["Catalog", "Sharing", "Lineage"].iter().enumerate() {
            let epic_ref = i;
            nodes.push(node(epic_ref, TASK, &format!("{epic} service"), APP, &[]));
            i += 1;
            for t in 1..=15 {
                let ph = if t <= 5 * (e + 1) / 2 { DONE } else { APP };
                let cond: &[&str] = if ph == DONE {
                    &[]
                } else if t == 5 * (e + 1) / 2 + 1 {
                    &["Ready"]
                } else {
                    &["Blocked"]
                };
                nodes.push(node(
                    i,
                    TASK,
                    &format!("{epic}: migrate step {t}"),
                    ph,
                    cond,
                ));
                parents.push((i, epic_ref));
                if t > 1 {
                    edges.push(edge(schema::BLOCKED_BY, i, i - 1));
                }
                i += 1;
            }
        }
        state(nodes, &parents, edges, Some(APPROVED), &[])
    }

    fn golden(name: &str, actual: &str) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("golden")
            .join(format!("{name}.md"));
        if std::env::var_os("GOLDEN_UPDATE").is_some() {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, actual).unwrap();
            return;
        }
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("no golden {}; run with GOLDEN_UPDATE=1", path.display()));
        assert_eq!(
            actual, expected,
            "{name} changed; rerun with GOLDEN_UPDATE=1 if that's intended"
        );
    }

    fn text_client() -> Client {
        Client {
            apps: false,
            control_room: Some("http://localhost:4330".into()),
        }
    }

    fn apps_client() -> Client {
        Client {
            apps: true,
            control_room: Some("http://localhost:4330".into()),
        }
    }

    fn diff() -> Vec<DiffEntry> {
        let e = |i: usize, kind: &str, change, label: &str| DiffEntry {
            key: r(i),
            kind: kind.into(),
            change,
            label: label.into(),
            content_changed: false,
            from_parent: None,
            to_parent: None,
        };
        vec![
            e(5, DECISION, Change::Added, "Where do revisions live?"),
            e(4, TASK, Change::Modified, "Control-room UI"),
            e(9, TASK, Change::Removed, "Drop the GitHub tracker"),
            DiffEntry {
                from_parent: Some(r(4)),
                to_parent: Some(r(0)),
                content_changed: true,
                ..e(6, GATE, Change::Moved, "Wait for the core PR")
            },
        ]
    }

    // -- the size rule --

    #[test]
    fn shape_layers_by_longest_path() {
        // A chain: one wide, three deep.
        assert_eq!(shape(3, &[(0, 1), (1, 2)]), Shape { width: 1, depth: 3 });
        // A diamond: two wide in the middle.
        assert_eq!(
            shape(4, &[(0, 1), (0, 2), (1, 3), (2, 3)]),
            Shape { width: 2, depth: 3 }
        );
        // A fan-out of five and of seven.
        let fan = |n: usize| (1..=n).map(|i| (0, i)).collect::<Vec<_>>();
        assert_eq!(shape(6, &fan(5)), Shape { width: 5, depth: 2 });
        assert_eq!(shape(8, &fan(7)), Shape { width: 7, depth: 2 });
        // Disconnected items share the first rank.
        assert_eq!(shape(5, &[]), Shape { width: 5, depth: 1 });
        // A node sits below its lowest predecessor, not its first.
        assert_eq!(
            shape(4, &[(0, 1), (1, 2), (0, 3), (2, 3)]),
            Shape { width: 1, depth: 4 }
        );
        // A cycle doesn't hang.
        assert_eq!(shape(2, &[(0, 1), (1, 0)]).depth, 1);
        assert_eq!(shape(0, &[]), Shape { width: 0, depth: 0 });
    }

    #[test]
    fn layout_follows_the_size_rule() {
        let s = |width, depth| Shape { width, depth };
        assert_eq!(layout(10, s(4, 3)), Layout::Graph(Direction::TopDown));
        assert_eq!(layout(10, s(5, 4)), Layout::Graph(Direction::LeftRight));
        assert_eq!(layout(10, s(6, 2)), Layout::Graph(Direction::LeftRight));
        assert_eq!(
            layout(20, s(6, 5)),
            Layout::Collapsed { width: 6, depth: 5 }
        );
        assert_eq!(
            layout(10, s(7, 2)),
            Layout::TableOnly(Omitted::TooWide { width: 7 })
        );
        // Narrow but too many.
        assert_eq!(
            layout(41, s(1, 41)),
            Layout::TableOnly(Omitted::TooMany { nodes: 41 })
        );
        assert_eq!(layout(40, s(1, 40)), Layout::Graph(Direction::TopDown));
        assert_eq!(layout(0, s(0, 0)), Layout::TableOnly(Omitted::Empty));
    }

    #[test]
    fn containers_stand_for_their_leaves() {
        // An epic with two tasks, blocking a third task: the tasks rank
        // first, the blocked one second.
        let st = state(
            vec![
                plan_node("P", 0.0),
                node(1, TASK, "Epic", APP, &[]),
                node(2, TASK, "a", APP, &[]),
                node(3, TASK, "b", APP, &[]),
                node(4, TASK, "after", APP, &[]),
            ],
            &[(2, 1), (3, 1)],
            vec![edge(schema::BLOCKED_BY, 4, 1)],
            None,
            &[],
        );
        let o = Outline::new(&st.graph);
        let scene = Scene::new(&o, &st.graph.root);
        assert_eq!(scene.leaves.len(), 3);
        assert_eq!(
            shape(scene.leaves.len(), &scene.deps),
            Shape { width: 2, depth: 2 }
        );
        let text = plan(&st, None, &Client::default());
        assert!(text.contains("subgraph T1[\"T1: Epic\"]"), "{text}");
        assert!(text.contains("T1 --> T4"), "{text}");
        assert!(text.contains("| parent |"), "{text}");
    }

    // -- labels and escaping --

    #[test]
    fn titles_are_cut_to_24_characters() {
        assert_eq!(cut("Short", LABEL_CHARS), "Short");
        let exact = "a".repeat(24);
        assert_eq!(cut(&exact, LABEL_CHARS), exact);
        let long = cut("Serve factory.v1 over Connect and gRPC", LABEL_CHARS);
        assert_eq!(long, "Serve factory.v1 over C…");
        assert_eq!(long.chars().count(), 24);
        // On characters, not bytes.
        let wide = cut(&"é".repeat(30), LABEL_CHARS);
        assert_eq!(wide.chars().count(), 24);
        // No space before the ellipsis.
        assert_eq!(
            cut("aaaaaaaaaaaaaaaaaaaaaa bbbbbbb", LABEL_CHARS),
            "aaaaaaaaaaaaaaaaaaaaaa…"
        );
        assert_eq!(
            label("T12", "Build the core on olai-store, fast"),
            "T12: Build the core on olai-…"
        );
    }

    #[test]
    fn mermaid_labels_escape_syntax() {
        assert_eq!(mermaid_text("say \"hi\""), "say #quot;hi#quot;");
        assert_eq!(
            mermaid_text("a [b] (c) {d}"),
            "a #91;b#93; #40;c#41; #123;d#125;"
        );
        assert_eq!(mermaid_text("a | b"), "a #124; b");
        assert_eq!(mermaid_text("<b>#1</b>"), "#lt;b#gt;#35;1#lt;/b#gt;");
        assert_eq!(mermaid_text("one\ntwo\r\n\tthree"), "one two three");
        assert_eq!(mermaid_text("`code`"), "#96;code#96;");
        // `#` first, so the codes aren't escaped again.
        assert_eq!(mermaid_text("#quot;"), "#35;quot;");
        // Cut before escaping: an entity is never cut in half.
        let l = label("T1", &"\"".repeat(30));
        assert!(l.ends_with("#quot;…"), "{l}");
    }

    #[test]
    fn table_cells_escape_pipes_and_newlines() {
        assert_eq!(cell("a | b\nc"), "a \\| b c");
        assert_eq!(cell(""), "–");
    }

    #[test]
    fn links_carry_the_plan() {
        let c = |u: &str| Client {
            apps: false,
            control_room: Some(u.into()),
        };
        assert_eq!(
            link(&c("http://h:4330"), "olai:x").unwrap(),
            "http://h:4330/?plan=olai%3Ax"
        );
        assert_eq!(
            link(&c("http://h/factory/"), "r").unwrap(),
            "http://h/factory/?plan=r"
        );
        assert_eq!(
            link(&c("http://h/?t=1"), "r").unwrap(),
            "http://h/?t=1&plan=r"
        );
        assert!(link(&Client::default(), "r").is_none());
    }

    // -- the results --

    #[test]
    fn plan_read_small() {
        let st = small();
        let text = plan(&st, None, &text_client());
        assert!(text.contains("```mermaid\nflowchart TD"), "{text}");
        for n in &st.graph.nodes[1..] {
            assert!(text.contains(&n.r#ref), "{text}");
        }
        golden("plan_read_small", &text);
    }

    #[test]
    fn plan_read_wide() {
        let text = plan(&wide(), None, &text_client());
        assert!(text.contains("flowchart LR"), "{text}");
        golden("plan_read_wide", &text);
    }

    #[test]
    fn plan_read_wide_and_deep_collapses() {
        let mut st = wide();
        // Three more ranks below the suite.
        for i in 9..12 {
            st.graph
                .nodes
                .push(node(i, TASK, &format!("Step {i}"), PROP, &["Blocked"]));
            st.graph.edges.push(edge(schema::CHILD_OF, i, 0));
            st.graph.edges.push(edge(schema::BLOCKED_BY, i, i - 1));
        }
        let text = plan(&st, None, &Client::default());
        assert!(text.contains("collapsed to one node per phase"), "{text}");
        golden("plan_read_collapsed", &text);
    }

    #[test]
    fn plan_read_large() {
        let st = large();
        let text = plan(&st, None, &text_client());
        assert!(!text.contains("```mermaid"), "{text}");
        assert!(
            text.contains("Graph left out: 48 items, more than 40"),
            "{text}"
        );
        assert!(text.contains("http://localhost:4330/?plan="), "{text}");
        golden("plan_read_large", &text);
    }

    #[test]
    fn plan_read_under_draws_the_subtree() {
        let st = large();
        let epic = r(17);
        let text = plan(&st, Some(&epic), &Client::default());
        assert!(
            text.starts_with("## T17: Sharing service (part of the plan"),
            "{text}"
        );
        assert!(text.contains("```mermaid"), "{text}");
        assert!(text.contains("**Items** (15)"), "{text}");
        assert!(!text.contains("Catalog"), "{text}");
        golden("plan_read_under", &text);
    }

    #[test]
    fn ready_lists_what_can_start() {
        golden("ready", &ready(&small()));
        assert!(
            ready(&state(vec![plan_node("P", 0.0)], &[], vec![], None, &[]))
                .contains("Nothing can start now")
        );
    }

    #[test]
    fn item_read_shows_everything() {
        let st = small();
        let mut n = st.graph.nodes[2].clone();
        n.spec = payload::spec(
            "Build the core on olai-store",
            "Writes go through the schema.",
            &wg::Task {
                repo: "open-lakehouse/luminescent-lakehouse".into(),
                profile: "executor".into(),
                ..payload::task(
                    "Writes are validated against the schema,\nand every change records a revision.",
                    &["crates/factory-core/**"],
                )
            },
        )
        .into();
        n.metadata = wg::NodeMetadata {
            labels: [("area".to_owned(), "core".to_owned())].into(),
            ..Default::default()
        }
        .into();
        let annotations = vec![wg::Annotation {
            r#ref: "olai:ann-1".into(),
            node: n.r#ref.clone(),
            kind: schema::LEARNING.into(),
            payload: payload::pack(&wg::Learning {
                lesson: "Index edges by kind before ranking.".into(),
                evidence: vec!["PR #14".into()],
                ..Default::default()
            })
            .into(),
            provenance: wg::Provenance {
                actor: "factory-planner".into(),
                ..Default::default()
            }
            .into(),
            ..Default::default()
        }];
        let runs = vec![wg::Run {
            r#ref: "olai:run-1".into(),
            node: n.r#ref.clone(),
            attempt: 1,
            state: wg::RunState::RUN_STATE_COMPLETED.into(),
            executor: "task.v1".into(),
            stage: "done".into(),
            cost_usd: 1.25,
            artifacts: vec![wg::Artifact {
                artifact: Some(wg::artifact::Artifact::PullRequest(Box::new(
                    wg::PullRequest {
                        url: "https://github.com/open-lakehouse/luminescent-lakehouse/pull/14"
                            .into(),
                        repo: "open-lakehouse/luminescent-lakehouse".into(),
                        number: 14,
                        state: "merged".into(),
                        ..Default::default()
                    },
                ))),
                ..Default::default()
            }],
            ..Default::default()
        }];
        let attention = vec![wg::Attention {
            r#ref: "olai:att-1".into(),
            kind: wg::AttentionKind::ATTENTION_KIND_PROPOSAL.into(),
            node: n.r#ref.clone(),
            summary: "Split the core into two tasks".into(),
            resolved: true,
            resolution: "accept".into(),
            resolved_by: "operator".into(),
            comment: "Go ahead.".into(),
            ..Default::default()
        }];
        let text = item(
            &st,
            &n,
            &ItemRecords {
                annotations: &annotations,
                runs: &runs,
                attention: &attention,
            },
        );
        golden("item_read", &text);
    }

    #[test]
    fn submit_links_the_control_room() {
        let st = small();
        let text = submitted(&st, &diff(), Some("olai:att-2"), &text_client());
        assert!(text.contains(REV), "{text}");
        assert!(text.contains("http://localhost:4330/?plan="), "{text}");
        golden("submit", &text);
        // An MCP Apps client is pointed at plan review instead.
        let apps = submitted(&st, &diff(), None, &apps_client());
        assert!(apps.contains("open_plan_review"), "{apps}");
        assert!(!apps.contains("http://"), "{apps}");
        // Without a link, it says where to go.
        let none = submitted(&st, &[], None, &Client::default());
        assert!(none.contains("in the factory's control room"), "{none}");
        assert!(none.contains("none since the approved revision"), "{none}");
    }

    #[test]
    fn open_plan_review_per_client() {
        let st = small();
        let apps = review(&st, &diff(), &apps_client());
        assert!(apps.starts_with("Opened the review"), "{apps}");
        assert!(apps.contains("Nothing is approved"), "{apps}");
        assert!(!apps.contains("http://"), "{apps}");
        golden("open_plan_review_apps", &apps);
        let text = review(&st, &diff(), &Client::default());
        assert!(text.contains("can't be shown in this client"), "{text}");
        golden("open_plan_review_text", &text);
    }

    #[test]
    fn propose_says_where_to_decide() {
        let text = proposed(&r(2), "olai:att-3", REV, &r(0), &text_client());
        assert!(text.contains("olai:att-3") && text.contains(REV), "{text}");
        golden("propose", &text);
        let apps = proposed(&r(2), "olai:att-3", REV, &r(0), &apps_client());
        assert!(!apps.contains("control room"), "{apps}");
    }

    #[test]
    fn changes_when_never_approved() {
        let mut st = small();
        st.approved = None;
        let text = changes(&st, &diff()[..1]);
        assert!(
            text.contains("never approved, so every item is new (1 item)"),
            "{text}"
        );
    }
}
