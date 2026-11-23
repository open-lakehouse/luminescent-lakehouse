//! The standalone server's Postgres store: refused until migrated, migrated
//! by `--migrate`, and kept across restarts; and its change feed, which
//! carries one server's writes to another's watchers.
//!
//! On a database of its own on `FACTORY_TEST_DATABASE_URL`'s server (`just
//! factory-test` starts one). Skipped without it, except in CI.
#![cfg(feature = "bin")]

use std::sync::Arc;
use std::time::Duration;

use connectrpc::client::{ClientConfig, HttpClient};
use factory_core::feed::{self, ListenConfig};
use factory_core::payload::{self, task};
use factory_core::pg::{Credentials, PgPool, PgSource, StaticCredentials};
use factory_core::schema::{PLAN, TASK};
use factory_core::{Extensions, Factory, GraphChange, Provenance, Schema, olai};
use factory_proto::connect_gen::factory::v1::FactoryServiceClient;
use factory_proto::v1;
use factory_proto::workgraph::v1 as wg;
use factory_server::cli::{Watch, Watcher, open_backend, watch};
use tokio::sync::broadcast;

/// A fresh database on the test server, and its URL.
async fn fresh_db() -> Option<String> {
    let Ok(base) = std::env::var("FACTORY_TEST_DATABASE_URL") else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must run the Postgres tests: set FACTORY_TEST_DATABASE_URL"
        );
        eprintln!("skipped: set FACTORY_TEST_DATABASE_URL to run on Postgres");
        return None;
    };
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    // Tests run in parallel, and the clock can repeat within a process.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = format!("factory_server_{}_{nanos}_{n}", std::process::id());
    let server = PgPool::connect(&base).await.unwrap();
    sqlx::query(&format!("CREATE DATABASE {name}"))
        .execute(&server)
        .await
        .unwrap();
    server.close().await;
    let (prefix, _) = base.rsplit_once('/').unwrap();
    Some(format!("{prefix}/{name}"))
}

#[tokio::test]
async fn migrates_on_request_and_keeps_the_graph() {
    let Some(url) = fresh_db().await else { return };
    let schema = Schema::builtin();

    let err = open_backend(&url, false, &schema, &StaticCredentials)
        .await
        .err()
        .expect("an unmigrated store is refused");
    assert!(err.to_string().contains("isn't applied"), "{err}");

    let backend = open_backend(&url, true, &schema, &StaticCredentials)
        .await
        .unwrap();
    let factory = Arc::new(Factory::new(backend, Extensions::builtin()));
    let root = factory_core::demo::seed(&factory).await.unwrap();

    // A restart: migrating again is a no-op, and the plan is still there.
    let backend = open_backend(&url, true, &schema, &StaticCredentials)
        .await
        .unwrap();
    let factory = Factory::new(backend, Extensions::builtin());
    let graphs = factory.list_graphs().await.unwrap();
    assert_eq!(graphs.len(), 1);
    assert_eq!(graphs[0].root.r#ref, root);
}

/// A factory on the store at `url`, over a pool of its own, as a separate
/// server would have.
async fn server(url: &str, migrate: bool) -> Arc<Factory> {
    let schema = Schema::builtin();
    let backend = open_backend(url, migrate, &schema, &StaticCredentials)
        .await
        .unwrap();
    Arc::new(Factory::new(backend, Extensions::builtin()))
}

fn by(actor: &str) -> Provenance {
    Provenance::new(actor, "test")
}

async fn plan(f: &Factory) -> String {
    f.create_node(
        None,
        PLAN,
        payload::spec("P", "", &wg::Plan::default()),
        Default::default(),
        &by("a"),
    )
    .await
    .unwrap()
    .r#ref
}

async fn add_task(f: &Factory, root: &str, title: &str) {
    f.create_node(
        Some(root),
        TASK,
        payload::spec(title, "", &task(title, &[])),
        Default::default(),
        &by("a"),
    )
    .await
    .unwrap();
}

/// The next change to `root`, within a few seconds.
async fn next(rx: &mut broadcast::Receiver<GraphChange>, root: &str) -> GraphChange {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let change = rx.recv().await.unwrap();
            if change.root == root {
                return change;
            }
        }
    })
    .await
    .expect("a change within 10s")
}

/// Wait until the listener's status satisfies `done`.
async fn until(listener: &feed::Listener, done: impl Fn(&feed::ListenStatus) -> bool) {
    let mut status = listener.status();
    tokio::time::timeout(Duration::from_secs(10), status.wait_for(|s| done(s)))
        .await
        .expect("the listener's status within 10s")
        .unwrap();
}

#[tokio::test]
async fn a_write_on_one_server_reaches_watch_graph_on_another() {
    let Some(url) = fresh_db().await else { return };
    let a = server(&url, true).await;
    let b = server(&url, false).await;
    let creds: Arc<dyn Credentials> = Arc::new(StaticCredentials);
    let watcher = watch(&b, Watch::Notify, &url, None, Duration::from_secs(1), creds)
        .await
        .unwrap();
    assert!(matches!(watcher, Watcher::Listen(_)));

    let served = b.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, factory_server::router(served))
            .await
            .unwrap();
    });
    let client = FactoryServiceClient::new(
        HttpClient::plaintext(),
        ClientConfig::new(format!("http://{addr}").parse().unwrap()),
    );

    // B hears of the new graph before anything watches it.
    let mut rx = b.subscribe();
    let root = plan(&a).await;
    next(&mut rx, &root).await;
    let mut stream = client
        .watch_graph(v1::WatchGraphRequest {
            root: root.clone(),
            ..Default::default()
        })
        .await
        .unwrap();
    let first = stream.message().await.unwrap().unwrap().to_owned_message();

    // A's write reaches B's watcher, at A's revision.
    add_task(&a, &root, "From A").await;
    let msg = tokio::time::timeout(Duration::from_secs(10), stream.message())
        .await
        .expect("A's change within 10s")
        .unwrap()
        .unwrap()
        .to_owned_message();
    assert_ne!(msg.revision, first.revision);
    assert_eq!(msg.revision, a.graph(&root).await.unwrap().revision);
    assert!(msg.sequence > first.sequence);

    // A run A records arrives with the run, read from the store.
    let run = wg::Run {
        r#ref: olai::new_ref(),
        node: root.clone(),
        stage: "fixing".into(),
        ..Default::default()
    };
    a.put_run(&run, &by("reconciler")).await.unwrap();
    let msg = tokio::time::timeout(Duration::from_secs(10), stream.message())
        .await
        .expect("A's run within 10s")
        .unwrap()
        .unwrap()
        .to_owned_message();
    match msg.change {
        Some(v1::watch_graph_response::Change::Run(r)) => {
            assert_eq!(
                (r.r#ref.as_str(), r.stage.as_str()),
                (run.r#ref.as_str(), "fixing")
            );
        }
        other => panic!("expected the run, got {other:?}"),
    }

    // B's own write reaches its watcher once: the listener skips its echo.
    add_task(&b, &root, "From B").await;
    let own = tokio::time::timeout(Duration::from_secs(10), stream.message())
        .await
        .expect("B's change")
        .unwrap()
        .unwrap()
        .to_owned_message();
    assert_eq!(own.revision, b.graph(&root).await.unwrap().revision);
    let echo = tokio::time::timeout(Duration::from_millis(500), stream.message()).await;
    assert!(echo.is_err(), "B's own write came back: {echo:?}");
}

#[tokio::test]
async fn nothing_is_lost_across_a_dropped_listen_connection() {
    let Some(url) = fresh_db().await else { return };
    let a = server(&url, true).await;
    let b = server(&url, false).await;
    let config = ListenConfig {
        retry: Duration::from_secs(2),
        ..Default::default()
    };
    let listener = feed::listen(
        b.clone(),
        PgSource::parse(&url).unwrap(),
        Arc::new(StaticCredentials),
        config,
    )
    .await
    .unwrap();
    let mut rx = b.subscribe();
    let root = plan(&a).await;
    next(&mut rx, &root).await;

    add_task(&a, &root, "Before").await;
    next(&mut rx, &root).await;

    // Drop the listen connection from the server side, and write while it's
    // gone: the reconnect catches up from the cursor.
    let admin = PgPool::connect(&url).await.unwrap();
    let killed: Vec<(bool,)> = sqlx::query_as(
        "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
         WHERE application_name = $1 AND datname = current_database()",
    )
    .bind(feed::APPLICATION_NAME)
    .fetch_all(&admin)
    .await
    .unwrap();
    assert_eq!(killed, vec![(true,)]);
    until(&listener, |s| !s.listening).await;
    add_task(&a, &root, "While away").await;
    let missed = a.graph(&root).await.unwrap().revision;
    until(&listener, |s| s.listening && s.connects == 2).await;
    let change = next(&mut rx, &root).await;
    assert_eq!(change.revision, missed);

    // And it listens again.
    add_task(&a, &root, "After").await;
    let change = next(&mut rx, &root).await;
    assert_eq!(change.revision, a.graph(&root).await.unwrap().revision);
}

#[tokio::test]
async fn poll_mode_gives_the_same_events() {
    let Some(url) = fresh_db().await else { return };
    let a = server(&url, true).await;
    let b = server(&url, false).await;
    let creds: Arc<dyn Credentials> = Arc::new(StaticCredentials);
    let _watcher = watch(
        &b,
        Watch::Poll,
        &url,
        None,
        Duration::from_millis(100),
        creds,
    )
    .await
    .unwrap();
    let mut rx = b.subscribe();
    let root = plan(&a).await;
    next(&mut rx, &root).await;

    add_task(&a, &root, "From A").await;
    let change = next(&mut rx, &root).await;
    assert_eq!(change.revision, a.graph(&root).await.unwrap().revision);

    let run = wg::Run {
        r#ref: olai::new_ref(),
        node: root.clone(),
        stage: "in_review".into(),
        ..Default::default()
    };
    a.put_run(&run, &by("reconciler")).await.unwrap();
    let change = next(&mut rx, &root).await;
    assert_eq!(change.run.map(|r| r.stage), Some("in_review".into()));

    // B's own writes aren't sent twice.
    add_task(&b, &root, "From B").await;
    next(&mut rx, &root).await;
    let echo = tokio::time::timeout(Duration::from_millis(500), next(&mut rx, &root)).await;
    assert!(echo.is_err(), "B's own write came back: {echo:?}");
}
