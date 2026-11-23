//! The change feed across processes: how a write in one process reaches the
//! [`WatchGraph`](crate::Factory::subscribe) watchers of another.
//!
//! A [`Factory`] over a backend with a change feed (the Postgres one) writes
//! each graph's latest change as a [`FeedEntry`] after every change it tells
//! its own watchers about. The entry's sequence increases per graph, in
//! commit order. On Postgres, committing an entry notifies channel
//! [`CHANNEL`] from inside the transaction, with a small payload: the root,
//! the sequence and the writer's [`origin`](crate::Factory::origin).
//!
//! A [`Follower`] turns the feed back into [`GraphChange`](crate::GraphChange)s
//! on the factory's broadcast. It remembers the last sequence it saw of every
//! graph, skips its own factory's entries (that factory already sent them),
//! and reads the rest from the store: the revision, and the run or attention
//! item the change carried. Two ways feed it:
//!
//! - [`listen`] (feature `postgres`): one dedicated `LISTEN` connection, on a
//!   direct host (a transaction pooler accepts `LISTEN` and never delivers).
//!   On every (re)connect it listens first, then reads the cursor once, so
//!   nothing committed while it was away is lost.
//! - [`poll`] (feature `postgres`): reads the same cursor on an interval, for
//!   a Postgres where `LISTEN` isn't available.
//!
//! Backends without a feed (SQLite, memory) only have in-process watchers.

use std::collections::HashMap;
use std::sync::Arc;

use serde::Deserialize;

use crate::backend::FeedEntry;
use crate::{Factory, Result};

/// The channel the Postgres store notifies when a feed entry commits.
pub const CHANNEL: &str = "factory";

/// A notification's payload on [`CHANNEL`].
#[derive(Debug, Deserialize)]
struct Notice {
    root: String,
    seq: u64,
    #[serde(default)]
    origin: String,
}

/// Feeds a factory's watchers the changes other processes made, from the
/// backend's change feed.
pub struct Follower {
    factory: Arc<Factory>,
    /// The last sequence seen of each graph, by root.
    seen: HashMap<String, u64>,
}

impl Follower {
    /// A follower of `factory`'s feed that has seen everything committed so
    /// far: [`catch_up`](Self::catch_up) sends what comes after.
    pub async fn new(factory: Arc<Factory>) -> Result<Self> {
        let seen = factory
            .feed(None)
            .await?
            .into_iter()
            .map(|(seq, entry)| (entry.root, seq))
            .collect();
        Ok(Self { factory, seen })
    }

    /// Read the cursor and send every change newer than the ones seen that
    /// another process made. Returns how many it sent.
    pub async fn catch_up(&mut self) -> Result<usize> {
        self.read(None).await
    }

    /// A notification's payload: reads the graph's entry and sends it, unless
    /// it was seen or is this factory's own. A payload that isn't a feed
    /// notice is ignored. Returns how many changes it sent.
    pub async fn heard(&mut self, payload: &str) -> Result<usize> {
        let notice: Notice = match serde_json::from_str(payload) {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!(payload, error = %e, "not a change feed notice");
                return Ok(0);
            }
        };
        if self.seen(&notice.root, notice.seq) {
            return Ok(0);
        }
        if notice.origin == self.factory.origin() {
            // This factory sent it when it wrote it. Notifications come in
            // commit order, so every earlier change to the graph came first.
            self.seen.insert(notice.root, notice.seq);
            return Ok(0);
        }
        self.read(Some(&notice.root)).await
    }

    fn seen(&self, root: &str, seq: u64) -> bool {
        self.seen.get(root).is_some_and(|s| *s >= seq)
    }

    async fn read(&mut self, root: Option<&str>) -> Result<usize> {
        let mut sent = 0;
        for (seq, entry) in self.factory.feed(root).await? {
            if self.seen(&entry.root, seq) {
                continue;
            }
            self.seen.insert(entry.root.clone(), seq);
            if entry.origin != self.factory.origin() {
                self.send(entry).await;
                sent += 1;
            }
        }
        Ok(sent)
    }

    async fn send(&self, entry: FeedEntry) {
        // The run and the attention item as the store has them now. A graph
        // deleted since has neither; the change still goes out.
        let run = match entry.run.as_str() {
            "" => None,
            r => self.factory.run(&entry.root, r).await.ok(),
        };
        let attention = match entry.attention.as_str() {
            "" => None,
            a => self
                .factory
                .attention(Some(&entry.root), true)
                .await
                .ok()
                .and_then(|items| items.into_iter().find(|i| i.r#ref == a)),
        };
        tracing::debug!(root = %entry.root, origin = %entry.origin, "change from another process");
        self.factory
            .send(&entry.root, &entry.revision, run, attention);
    }
}

#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "postgres")]
pub use postgres::{APPLICATION_NAME, ListenConfig, ListenStatus, Listener, listen, poll};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Extensions, Provenance, Schema, olai};
    use factory_proto::workgraph::v1 as wg;

    /// Two factories on one store with a change feed, as two processes would
    /// share a database.
    fn pair() -> (Arc<Factory>, Arc<Factory>) {
        let schema = Schema::builtin();
        let backend = Arc::new(olai::in_memory(&schema).with_change_feed());
        (
            Arc::new(Factory::new(backend.clone(), Extensions::builtin())),
            Arc::new(Factory::new(backend, Extensions::builtin())),
        )
    }

    async fn plan(f: &Factory, title: &str) -> wg::Node {
        f.create_node(
            None,
            crate::schema::PLAN,
            wg::NodeSpec {
                title: title.into(),
                ..Default::default()
            },
            Default::default(),
            &Provenance::new("test", "test"),
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn a_follower_sends_other_factories_changes_once() {
        let (a, b) = pair();
        let mut follower = Follower::new(b.clone()).await.unwrap();
        let mut rx = b.subscribe();

        let root = plan(&a, "one").await;
        assert_eq!(follower.catch_up().await.unwrap(), 1);
        let change = rx.try_recv().unwrap();
        assert_eq!(change.root, root.r#ref);
        assert_eq!(
            change.revision,
            a.graph(&root.r#ref).await.unwrap().revision
        );
        assert_eq!(follower.catch_up().await.unwrap(), 0, "seen already");

        // B's own write reaches its watchers directly, and the follower
        // doesn't send it again.
        plan(&b, "two").await;
        assert!(rx.try_recv().is_ok());
        assert_eq!(follower.catch_up().await.unwrap(), 0);
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_follower_starts_from_what_is_committed() {
        let (a, b) = pair();
        plan(&a, "before").await;
        let mut follower = Follower::new(b).await.unwrap();
        assert_eq!(follower.catch_up().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn notices_skip_own_and_seen_changes_and_carry_the_run() {
        let (a, b) = pair();
        let mut follower = Follower::new(b.clone()).await.unwrap();
        let mut rx = b.subscribe();
        let root = plan(&a, "one").await;
        let notice = |f: &Factory, seq: u64| {
            format!(
                r#"{{"root":"{}","seq":{seq},"origin":"{}"}}"#,
                root.r#ref,
                f.origin()
            )
        };
        let (seq, _) = a.feed(Some(&root.r#ref)).await.unwrap()[0].clone();
        assert_eq!(follower.heard(&notice(&a, seq)).await.unwrap(), 1);
        assert_eq!(follower.heard(&notice(&a, seq)).await.unwrap(), 0);
        assert_eq!(rx.try_recv().unwrap().root, root.r#ref);
        assert_eq!(follower.heard("not json").await.unwrap(), 0);

        let run = wg::Run {
            r#ref: olai::new_ref(),
            node: root.r#ref.clone(),
            ..Default::default()
        };
        a.put_run(&run, &Provenance::new("test", "test"))
            .await
            .unwrap();
        let (seq, entry) = a.feed(Some(&root.r#ref)).await.unwrap()[0].clone();
        assert_eq!(entry.run, run.r#ref);
        // A notice of B's own seq is only recorded; a later one of A's is
        // read from the store, with the run.
        assert_eq!(follower.heard(&notice(&a, seq)).await.unwrap(), 1);
        let change = rx.try_recv().unwrap();
        assert_eq!(change.run.map(|r| r.r#ref), Some(run.r#ref));
        assert_eq!(follower.heard(&notice(&b, seq + 1)).await.unwrap(), 0);
        assert!(follower.seen(&root.r#ref, seq + 1));
    }

    #[tokio::test]
    async fn without_a_feed_nothing_is_published() {
        let schema = Schema::builtin();
        let f = Arc::new(Factory::new(
            Arc::new(olai::in_memory(&schema)),
            Extensions::builtin(),
        ));
        plan(&f, "one").await;
        assert!(f.feed(None).await.unwrap().is_empty());
    }
}
