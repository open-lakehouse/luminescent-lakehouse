//! The Postgres feeds of a [`Follower`]: a dedicated `LISTEN` connection
//! ([`listen`]), or polling the cursor ([`poll`]).

use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::{PgListener, PgPoolOptions};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::{CHANNEL, Follower};
use crate::pg::{self, Credentials, PgSource};
use crate::{Error, Factory, Result};

/// The `application_name` of a listener's connection, to find it in
/// `pg_stat_activity`.
pub const APPLICATION_NAME: &str = "factory-watch";

/// How a [`Listener`] keeps its connection.
#[derive(Debug, Clone, Copy)]
pub struct ListenConfig {
    /// The wait before the first reconnect; it doubles on each failure, up
    /// to [`max_retry`](Self::max_retry).
    pub retry: Duration,
    pub max_retry: Duration,
    /// Ping the connection after this long without a notification, to find a
    /// connection that died without closing.
    pub ping: Duration,
}

impl Default for ListenConfig {
    fn default() -> Self {
        Self {
            retry: Duration::from_secs(1),
            max_retry: Duration::from_secs(30),
            ping: Duration::from_secs(30),
        }
    }
}

/// Where a [`Listener`] is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListenStatus {
    /// The connection is up and listening.
    pub listening: bool,
    /// How many times it has connected, the first time included.
    pub connects: u64,
    /// Why the connection was last lost, or a reconnect failed.
    pub error: Option<String>,
}

/// A running [`listen`]er. Dropping it stops it.
pub struct Listener {
    status: watch::Receiver<ListenStatus>,
    task: JoinHandle<()>,
}

impl Listener {
    /// The listener's status, as it changes.
    pub fn status(&self) -> watch::Receiver<ListenStatus> {
        self.status.clone()
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// `LISTEN` on [`CHANNEL`] over a connection of its own on `source`, and feed
/// what other processes write to `factory`'s watchers.
///
/// `source` must be a direct host: a pooled one is refused
/// ([`pg::refuse_pooler`]). The connection opens with `credentials`'
/// password each time, so a rotating token is current on every reconnect.
/// After a reconnect the listener reads the cursor once and sends what it
/// missed ([`Follower::catch_up`]).
///
/// The first connection opens before this returns, so a bad URL or password
/// fails here; later failures retry with backoff, and show in
/// [`Listener::status`].
pub async fn listen(
    factory: Arc<Factory>,
    source: PgSource,
    credentials: Arc<dyn Credentials>,
    config: ListenConfig,
) -> Result<Listener> {
    pg::refuse_pooler(&source)?;
    // Listen before reading the cursor: what commits in between is both
    // notified and read, and the follower sends it once.
    let listener = subscribe(&source, credentials.as_ref()).await?;
    let follower = Follower::new(factory).await?;
    let (tx, status) = watch::channel(ListenStatus {
        listening: true,
        connects: 1,
        error: None,
    });
    tracing::info!(db = %source.redacted(), channel = CHANNEL, "change feed listening");
    let task = tokio::spawn(run(listener, follower, source, credentials, config, tx));
    Ok(Listener { status, task })
}

async fn subscribe(source: &PgSource, credentials: &dyn Credentials) -> Result<PgListener> {
    let options = credentials
        .apply(source.options()?)
        .await?
        .application_name(APPLICATION_NAME);
    let failed = |e: sqlx::Error| {
        Error::internal(format!(
            "listening on {} for the change feed: {e}",
            source.redacted()
        ))
    };
    // A pool of one, for PgListener; a reconnect builds a new one with the
    // credentials' current password rather than reusing these options.
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(failed)?;
    let mut listener = PgListener::connect_with(&pool).await.map_err(failed)?;
    listener.eager_reconnect(false);
    listener.listen(CHANNEL).await.map_err(failed)?;
    Ok(listener)
}

/// Why a connection's loop ended.
async fn listening(listener: &mut PgListener, follower: &mut Follower, ping: Duration) -> String {
    loop {
        match tokio::time::timeout(ping, listener.try_recv()).await {
            Ok(Ok(Some(notice))) => {
                // A failed read would lose the change: start over, and catch
                // up from the cursor.
                if let Err(e) = follower.heard(notice.payload()).await {
                    return format!("reading a change: {e}");
                }
            }
            Ok(Ok(None)) => return "the connection closed".into(),
            Ok(Err(e)) => return e.to_string(),
            Err(_) => {
                if let Err(e) = sqlx::query("SELECT 1").execute(&mut *listener).await {
                    return format!("ping: {e}");
                }
            }
        }
    }
}

async fn run(
    mut listener: PgListener,
    mut follower: Follower,
    source: PgSource,
    credentials: Arc<dyn Credentials>,
    config: ListenConfig,
    status: watch::Sender<ListenStatus>,
) {
    loop {
        let lost = listening(&mut listener, &mut follower, config.ping).await;
        drop(listener);
        tracing::warn!(error = %lost, "change feed connection lost; reconnecting");
        status.send_modify(|s| {
            s.listening = false;
            s.error = Some(lost);
        });
        let mut wait = config.retry;
        listener = loop {
            tokio::time::sleep(wait).await;
            wait = (wait * 2).min(config.max_retry);
            let caught_up = async {
                let listener = subscribe(&source, credentials.as_ref()).await?;
                let sent = follower.catch_up().await?;
                Ok::<_, Error>((listener, sent))
            };
            match caught_up.await {
                Ok((listener, sent)) => {
                    tracing::info!(missed = sent, "change feed listening again");
                    status.send_modify(|s| {
                        s.listening = true;
                        s.connects += 1;
                    });
                    break listener;
                }
                Err(e) => {
                    tracing::warn!(error = %e, retry = ?wait, "change feed reconnect failed");
                    status.send_modify(|s| s.error = Some(e.to_string()));
                }
            }
        };
    }
}

/// Read the change feed's cursor every `every` and feed what other processes
/// wrote to `factory`'s watchers: [`listen`]'s fallback for a Postgres where
/// `LISTEN` isn't available (only a pooler, say). Reads go through the
/// factory's own store. The task runs until aborted.
pub async fn poll(factory: Arc<Factory>, every: Duration) -> Result<JoinHandle<()>> {
    let mut follower = Follower::new(factory).await?;
    tracing::info!(every = ?every, "change feed polling");
    Ok(tokio::spawn(async move {
        let mut tick = tokio::time::interval(every);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            if let Err(e) = follower.catch_up().await {
                tracing::warn!(error = %e, "change feed poll failed");
            }
        }
    }))
}
