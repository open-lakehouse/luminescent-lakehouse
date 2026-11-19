//! The code host: where PRs live. The reconciler polls it for the PRs of a
//! live run (state, draft, review decision, checks, merged), and
//! [`crate::artifact::merge_prs`] folds what it says into the run. There are
//! no webhooks yet, so polling is the whole interface.
//!
//! [`github::GitHub`] (feature `github`) is the GitHub adapter;
//! [`FakeCodeHost`] (feature `testing`) is for tests.

use async_trait::async_trait;

use crate::Result;

#[cfg(feature = "github")]
pub mod github;

/// A PR as the code host reports it, in the factory's vocabulary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PrStatus {
    pub url: String,
    pub title: String,
    pub head: String,
    pub base: String,
    /// `open`, `merged` or `closed`.
    pub state: String,
    pub draft: bool,
    /// `approved`, `changes_requested`, `review_required`, or empty.
    pub review_decision: String,
    /// The head commit's checks: `passing`, `failing`, `pending`, or empty
    /// when it has none.
    pub checks: String,
}

impl PrStatus {
    pub fn merged(&self) -> bool {
        self.state == "merged"
    }
}

#[async_trait]
pub trait CodeHost: Send + Sync + 'static {
    /// Whether `url` is a PR on this host.
    fn handles(&self, url: &str) -> bool;
    async fn pull_request(&self, url: &str) -> Result<PrStatus>;
}

/// A code host in memory, for tests: PRs are what the test sets.
#[cfg(any(test, feature = "testing"))]
#[derive(Default)]
pub struct FakeCodeHost {
    prs: std::sync::Mutex<std::collections::BTreeMap<String, PrStatus>>,
    polls: std::sync::atomic::AtomicUsize,
}

#[cfg(any(test, feature = "testing"))]
impl FakeCodeHost {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the PR at `status.url`.
    pub fn set(&self, status: PrStatus) {
        self.prs
            .lock()
            .expect("fake code host lock")
            .insert(status.url.clone(), status);
    }

    /// Update the PR at `url` (created open if unknown).
    pub fn update(&self, url: &str, f: impl FnOnce(&mut PrStatus)) {
        let mut prs = self.prs.lock().expect("fake code host lock");
        let pr = prs.entry(url.into()).or_insert_with(|| PrStatus {
            url: url.into(),
            state: "open".into(),
            ..Default::default()
        });
        f(pr);
    }

    /// Merge the PR at `url`.
    pub fn merge(&self, url: &str) {
        self.update(url, |p| p.state = "merged".into());
    }

    /// How many times a PR was read.
    pub fn polls(&self) -> usize {
        self.polls.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[cfg(any(test, feature = "testing"))]
#[async_trait]
impl CodeHost for FakeCodeHost {
    fn handles(&self, _url: &str) -> bool {
        true
    }

    async fn pull_request(&self, url: &str) -> Result<PrStatus> {
        self.polls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.prs
            .lock()
            .expect("fake code host lock")
            .get(url)
            .cloned()
            .ok_or_else(|| crate::Error::NotFound(format!("pull request {url}")))
    }
}
