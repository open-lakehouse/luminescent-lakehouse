//! The service directory and the proxy behind the UI's `ServiceBinding.fetch`.
//!
//! The UI addresses a service by its key (the API it serves, `unity-catalog`, whichever
//! implementation runs; else its trestle module id, `headwaters`; see
//! [`ServiceKeys`](crate::keys::ServiceKeys)) and a path; it never sees a base URL or a
//! credential. [`Proxy`] resolves the key through a
//! [`ServiceDirectory`], joins the path onto the upstream's base URL (refusing anything that
//! would leave it), adds the upstream's own headers (a gateway credential, a Neon Auth token)
//! and streams the response back.
//!
//! The scaffold binds nothing ([`StubDirectory`]); running instances (#18) and Neon branches
//! (#20) provide real directories.

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use futures::{Stream, StreamExt, TryStreamExt};
use reqwest::Url;
use serde::{Deserialize, Serialize};

/// Where a bound service runs (mirrors `@open-lakehouse/host`'s `Placement`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Placement {
    /// A compose module of a local instance.
    Local,
    /// A Neon Function on a branch.
    Remote,
}

/// A bound service, as the UI may see it: no URL, no credential.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ServiceInfo {
    pub module_id: String,
    pub placement: Placement,
}

/// Where the proxy sends a module's requests. Stays on the Rust side.
#[derive(Clone, Debug)]
pub struct Upstream {
    pub base_url: Url,
    pub placement: Placement,
    /// Added to every request, replacing any the UI sent under the same name.
    pub headers: Vec<(String, String)>,
}

impl Upstream {
    pub fn new(base_url: Url, placement: Placement) -> Self {
        Upstream {
            base_url,
            placement,
            headers: Vec::new(),
        }
    }
}

/// Which services the current environment binds.
pub trait ServiceDirectory: Send + Sync {
    fn list(&self) -> Vec<ServiceInfo>;
    fn resolve(&self, module_id: &str) -> Option<Upstream>;
}

/// The scaffold's directory: no environment is running, so nothing is bound.
#[derive(Clone, Copy, Debug, Default)]
pub struct StubDirectory;

impl ServiceDirectory for StubDirectory {
    fn list(&self) -> Vec<ServiceInfo> {
        Vec::new()
    }

    fn resolve(&self, _module_id: &str) -> Option<Upstream> {
        None
    }
}

/// Binds modules to fixed local URLs, for development and the smoke test, until
/// instances bind real ones: `module=url[,module=url…]`.
pub const SERVICES_VAR: &str = "LUMINESCENT_SERVICES";

/// A fixed set of bindings.
#[derive(Clone, Debug, Default)]
pub struct StaticDirectory(BTreeMap<String, Upstream>);

impl StaticDirectory {
    pub fn with(mut self, module_id: impl Into<String>, upstream: Upstream) -> Self {
        self.0.insert(module_id.into(), upstream);
        self
    }

    /// Parse a [`SERVICES_VAR`] value: `module=url` pairs, comma-separated. A loopback URL (a
    /// local instance's gateway) is [`Placement::Local`], any other (a Neon Function) is
    /// [`Placement::Remote`].
    pub fn parse(spec: &str) -> Result<Self, String> {
        spec.split(',')
            .map(str::trim)
            .filter(|pair| !pair.is_empty())
            .try_fold(StaticDirectory::default(), |dir, pair| {
                let (module_id, url) = pair
                    .split_once('=')
                    .ok_or_else(|| format!("`{pair}` is not module=url"))?;
                let url = Url::parse(url.trim()).map_err(|e| format!("`{pair}`: {e}"))?;
                if !matches!(url.scheme(), "http" | "https") {
                    return Err(format!("`{pair}`: not an http(s) URL"));
                }
                let placement = placement_of(&url);
                Ok(dir.with(module_id.trim(), Upstream::new(url, placement)))
            })
    }
}

/// Where a hand-bound URL runs: on this machine when its host is loopback.
fn placement_of(url: &Url) -> Placement {
    let host = url.host_str().unwrap_or_default();
    let ip = host.trim_start_matches('[').trim_end_matches(']');
    let local = host == "localhost"
        || host.ends_with(".localhost")
        || ip
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
    if local {
        Placement::Local
    } else {
        Placement::Remote
    }
}

/// Bearer tokens for remote services, until the shell signs in with Neon Auth:
/// `module=token[,module=token…]`, e.g. a service account's `olk_` API key for
/// `unity-catalog` (docs/desktop.md).
pub const TOKENS_VAR: &str = "LUMINESCENT_SERVICE_TOKENS";

/// The bearer token of each module, sent to its [`Placement::Remote`] upstreams
/// only. Never shown in `Debug` output.
#[derive(Clone, Default)]
pub struct ServiceTokens(BTreeMap<String, String>);

impl std::fmt::Debug for ServiceTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_set().entries(self.0.keys()).finish()
    }
}

impl ServiceTokens {
    pub fn with(mut self, module_id: impl Into<String>, token: impl Into<String>) -> Self {
        self.0.insert(module_id.into(), token.into());
        self
    }

    /// Parse a [`TOKENS_VAR`] value: `module=token` pairs, comma-separated. A token
    /// is everything after the first `=`.
    pub fn parse(spec: &str) -> Result<Self, String> {
        spec.split(',')
            .map(str::trim)
            .filter(|pair| !pair.is_empty())
            .try_fold(ServiceTokens::default(), |tokens, pair| {
                let (module_id, token) = pair
                    .split_once('=')
                    .map(|(m, t)| (m.trim(), t.trim()))
                    .filter(|(m, t)| !m.is_empty() && !t.is_empty())
                    .ok_or_else(|| {
                        // Never echo the pair: it holds the token.
                        let module = pair.split('=').next().unwrap_or_default().trim();
                        format!("the entry for `{module}` is not module=token")
                    })?;
                if !token.bytes().all(|b| b.is_ascii_graphic()) {
                    return Err(format!(
                        "the token for `{module_id}` is not printable ASCII"
                    ));
                }
                Ok(tokens.with(module_id, token))
            })
    }

    /// `upstream` with `module_id`'s token as its `Authorization`, when it is remote.
    pub fn apply(&self, module_id: &str, mut upstream: Upstream) -> Upstream {
        if upstream.placement == Placement::Remote
            && let Some(token) = self.0.get(module_id)
        {
            upstream
                .headers
                .retain(|(name, _)| !name.eq_ignore_ascii_case("authorization"));
            upstream
                .headers
                .push(("authorization".into(), format!("Bearer {token}")));
        }
        upstream
    }
}

impl ServiceDirectory for StaticDirectory {
    fn list(&self) -> Vec<ServiceInfo> {
        self.0
            .iter()
            .map(|(id, up)| ServiceInfo {
                module_id: id.clone(),
                placement: up.placement,
            })
            .collect()
    }

    fn resolve(&self, module_id: &str) -> Option<Upstream> {
        self.0.get(module_id).cloned()
    }
}

/// A request from the UI: a module and a path under it, never a URL.
#[derive(Clone, Debug, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProxyRequest {
    pub module_id: String,
    pub method: String,
    /// Absolute path under the module, with the query: `/api/2.1/unity-catalog/catalogs?max_results=10`.
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// The status line and headers, sent to the UI before the body streams.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ResponseHead {
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
}

/// Why a proxied request failed before (or while) a response arrived.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProxyError {
    /// The current environment binds no such module. The UI turns this into
    /// `ServiceUnavailableError`.
    #[error("no service is bound for module `{module_id}`")]
    Unbound { module_id: String },
    /// The UI sent something the proxy won't forward.
    #[error("invalid request: {message}")]
    InvalidRequest { message: String },
    /// The upstream couldn't be reached, or the body broke off.
    #[error("upstream error: {message}")]
    Upstream { message: String },
}

/// The response body, chunk by chunk.
pub type BodyStream = Pin<Box<dyn Stream<Item = Result<Bytes, ProxyError>> + Send>>;

/// Hop-by-hop headers, which describe one connection and are never forwarded
/// (RFC 9110 §7.6.1), plus those the client library sets itself.
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "content-length",
    "host",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

fn forwardable(name: &str) -> bool {
    !HOP_BY_HOP.iter().any(|h| name.eq_ignore_ascii_case(h))
}

/// Resolves modules and forwards requests to them.
#[derive(Clone)]
pub struct Proxy {
    client: reqwest::Client,
    directory: Arc<dyn ServiceDirectory>,
}

impl Proxy {
    pub fn new(directory: Arc<dyn ServiceDirectory>) -> Self {
        Proxy {
            client: reqwest::Client::new(),
            directory,
        }
    }

    pub fn services(&self) -> Vec<ServiceInfo> {
        self.directory.list()
    }

    /// The upstream and the URL `request` goes to.
    pub fn target(&self, request: &ProxyRequest) -> Result<(Upstream, Url), ProxyError> {
        let upstream =
            self.directory
                .resolve(&request.module_id)
                .ok_or_else(|| ProxyError::Unbound {
                    module_id: request.module_id.clone(),
                })?;
        let url = join_under(&upstream.base_url, &request.path)?;
        Ok((upstream, url))
    }

    /// Send `request`; the head arrives with the result, the body as a stream.
    pub async fn send(
        &self,
        request: ProxyRequest,
    ) -> Result<(ResponseHead, BodyStream), ProxyError> {
        let (upstream, url) = self.target(&request)?;
        let method = reqwest::Method::from_bytes(request.method.as_bytes()).map_err(|_| {
            ProxyError::InvalidRequest {
                message: format!("bad method `{}`", request.method),
            }
        })?;
        let mut builder = self.client.request(method, url);
        for (name, value) in &request.headers {
            let overridden = upstream
                .headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case(name));
            if forwardable(name) && !overridden {
                builder = builder.header(name, value);
            }
        }
        for (name, value) in &upstream.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = request.body {
            builder = builder.body(body);
        }
        let response = builder.send().await.map_err(upstream_error)?;
        let status = response.status();
        let head = ResponseHead {
            status: status.as_u16(),
            status_text: status.canonical_reason().unwrap_or_default().to_owned(),
            headers: response
                .headers()
                .iter()
                .filter(|(name, _)| forwardable(name.as_str()))
                .filter_map(|(name, value)| {
                    Some((name.as_str().to_owned(), value.to_str().ok()?.to_owned()))
                })
                .collect(),
        };
        let body = response.bytes_stream().map_err(upstream_error).boxed();
        Ok((head, body))
    }
}

fn upstream_error(e: reqwest::Error) -> ProxyError {
    ProxyError::Upstream {
        message: e.without_url().to_string(),
    }
}

/// `path` (with its query) under `base`, refusing anything that would leave `base`: another
/// origin, a scheme-relative path, or `..` above `base`'s path.
pub fn join_under(base: &Url, path: &str) -> Result<Url, ProxyError> {
    let invalid = |message: &str| ProxyError::InvalidRequest {
        message: format!("{message}: `{path}`"),
    };
    if !path.starts_with('/') || path.starts_with("//") || path.contains('\\') {
        return Err(invalid("path must be absolute under the module"));
    }
    let prefix = base.path().trim_end_matches('/');
    let mut url = base.clone();
    let (p, query) = match path.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (path, None),
    };
    // Setting the path normalizes `.` and `..` segments, so check the prefix afterwards.
    url.set_path(&format!("{prefix}{p}"));
    url.set_query(query);
    url.set_fragment(None);
    let within = url.path() == prefix
        || url
            .path()
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'));
    if url.origin() != base.origin() || !within {
        return Err(invalid("path leaves the module"));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    fn request(module_id: &str, path: &str) -> ProxyRequest {
        ProxyRequest {
            module_id: module_id.into(),
            method: "GET".into(),
            path: path.into(),
            headers: Vec::new(),
            body: None,
        }
    }

    #[test]
    fn stub_binds_nothing() {
        let proxy = Proxy::new(Arc::new(StubDirectory));
        assert!(proxy.services().is_empty());
        assert_eq!(
            proxy.target(&request("unity-catalog", "/")).unwrap_err(),
            ProxyError::Unbound {
                module_id: "unity-catalog".into()
            }
        );
    }

    #[test]
    fn joins_under_the_base_path() {
        let base = url("http://127.0.0.1:8080/uc/");
        assert_eq!(
            join_under(&base, "/api/2.1/catalogs?max_results=1")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:8080/uc/api/2.1/catalogs?max_results=1"
        );
        assert_eq!(
            join_under(&url("http://h"), "/x").unwrap().as_str(),
            "http://h/x"
        );
    }

    #[test]
    fn refuses_paths_that_leave_the_module() {
        let base = url("http://127.0.0.1:8080/uc");
        for path in [
            "relative",
            "//evil.example/x",
            "/../admin",
            "/a/../../admin",
            "/%2e%2e/admin",
            "\\evil",
            "http://evil.example/",
        ] {
            assert!(
                matches!(
                    join_under(&base, path),
                    Err(ProxyError::InvalidRequest { .. })
                ),
                "{path} should be refused, got {:?}",
                join_under(&base, path)
            );
        }
        // `..` that stays inside is fine.
        assert_eq!(
            join_under(&base, "/a/../b").unwrap().as_str(),
            "http://127.0.0.1:8080/uc/b"
        );
    }

    #[test]
    fn lists_without_urls() {
        let dir = StaticDirectory::default().with(
            "lineage",
            Upstream::new(url("http://127.0.0.1:1/"), Placement::Remote),
        );
        assert_eq!(
            dir.list(),
            vec![ServiceInfo {
                module_id: "lineage".into(),
                placement: Placement::Remote
            }]
        );
    }

    #[test]
    fn service_tokens_reach_remote_upstreams_only() {
        let tokens =
            ServiceTokens::parse(" unity-catalog = olk_ab_cd , headwaters=a.b-c_d ").unwrap();
        let remote = Upstream::new(url("https://uc.example/"), Placement::Remote);
        let local = Upstream::new(url("http://localhost:9080/"), Placement::Local);

        let uc = tokens.apply("unity-catalog", remote.clone());
        assert_eq!(
            uc.headers,
            [("authorization".to_string(), "Bearer olk_ab_cd".to_string())]
        );
        // Applied again, it replaces rather than adds.
        assert_eq!(tokens.apply("unity-catalog", uc).headers.len(), 1);
        assert!(tokens.apply("unity-catalog", local).headers.is_empty());
        assert!(tokens.apply("rustfs", remote).headers.is_empty());
        assert!(ServiceTokens::parse("").unwrap().0.is_empty());
    }

    #[test]
    fn service_token_errors_never_echo_the_token() {
        for spec in [
            "unity-catalog",
            "unity-catalog=",
            "=olk_secret",
            "unity-catalog=olk secret",
        ] {
            let err = ServiceTokens::parse(spec).unwrap_err();
            assert!(
                !err.contains("olk") && !err.contains("secret"),
                "{spec}: {err}"
            );
        }
        let tokens = ServiceTokens::parse("unity-catalog=olk_secret").unwrap();
        assert_eq!(format!("{tokens:?}"), r#"{"unity-catalog"}"#);
    }

    #[test]
    fn parses_the_services_override() {
        let dir = StaticDirectory::parse(" a=http://127.0.0.1:1/x , b=https://h ,").unwrap();
        assert_eq!(
            dir.list()
                .iter()
                .map(|s| s.module_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert_eq!(
            dir.resolve("a").unwrap().base_url.as_str(),
            "http://127.0.0.1:1/x"
        );
        assert_eq!(dir.list()[0].placement, Placement::Local);
        assert_eq!(dir.list()[1].placement, Placement::Remote);
        assert!(StaticDirectory::parse("nourl").is_err());
        assert!(StaticDirectory::parse("a=not a url").is_err());
        assert!(StaticDirectory::parse("a=file:///etc").is_err());
    }

    #[test]
    fn loopback_urls_are_local() {
        for (spec, placement) in [
            ("http://localhost:9080", Placement::Local),
            ("http://dev.localhost:9080", Placement::Local),
            ("http://127.0.0.1:9080", Placement::Local),
            ("http://[::1]:9080", Placement::Local),
            ("https://unitycatalog-x.fn.neon.tech", Placement::Remote),
            ("http://192.168.1.2:9080", Placement::Remote),
        ] {
            assert_eq!(placement_of(&url(spec)), placement, "{spec}");
        }
    }

    /// One HTTP/1.1 exchange: returns the raw request it read, answers with a chunked body.
    async fn serve_once(listener: TcpListener, chunks: &'static [&'static str]) -> String {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut buf = vec![0; 8192];
        let mut req = Vec::new();
        loop {
            let n = sock.read(&mut buf).await.unwrap();
            req.extend_from_slice(&buf[..n]);
            if n == 0 || req.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        let mut resp = String::from(
            "HTTP/1.1 201 Created\r\ncontent-type: text/plain\r\nx-upstream: yes\r\nconnection: close\r\ntransfer-encoding: chunked\r\n\r\n",
        );
        for c in chunks {
            resp.push_str(&format!("{:x}\r\n{c}\r\n", c.len()));
        }
        resp.push_str("0\r\n\r\n");
        sock.write_all(resp.as_bytes()).await.unwrap();
        String::from_utf8_lossy(&req).into_owned()
    }

    #[tokio::test]
    async fn forwards_and_streams() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(serve_once(listener, &["hello ", "streamed ", "world"]));

        let mut upstream = Upstream::new(url(&format!("http://{addr}/base/")), Placement::Local);
        upstream
            .headers
            .push(("authorization".into(), "Bearer secret".into()));
        let proxy = Proxy::new(Arc::new(StaticDirectory::default().with("svc", upstream)));

        let mut req = request("svc", "/items?q=1");
        req.headers = vec![
            ("accept".into(), "text/plain".into()),
            ("authorization".into(), "Bearer from-ui".into()),
            ("host".into(), "evil.example".into()),
        ];
        let (head, body) = proxy.send(req).await.unwrap();
        assert_eq!(head.status, 201);
        assert_eq!(head.status_text, "Created");
        assert!(head.headers.contains(&("x-upstream".into(), "yes".into())));
        assert!(
            !head
                .headers
                .iter()
                .any(|(n, _)| n == "transfer-encoding" || n == "connection"),
            "{:?}",
            head.headers
        );
        let chunks: Vec<Bytes> = body.try_collect().await.unwrap();
        assert_eq!(chunks.concat(), b"hello streamed world");

        let raw = server.await.unwrap().to_ascii_lowercase();
        assert!(raw.starts_with("get /base/items?q=1 http/1.1"), "{raw}");
        assert!(raw.contains("authorization: bearer secret"), "{raw}");
        assert!(!raw.contains("from-ui"), "{raw}");
        assert!(!raw.contains("evil.example"), "{raw}");
        assert!(raw.contains("accept: text/plain"), "{raw}");
    }

    #[tokio::test]
    async fn unreachable_upstream_is_an_upstream_error() {
        // Bind and drop, so the port is (very likely) closed.
        let addr = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap();
        let proxy = Proxy::new(Arc::new(StaticDirectory::default().with(
            "svc",
            Upstream::new(url(&format!("http://{addr}/")), Placement::Local),
        )));
        let err = proxy.send(request("svc", "/")).await.err().unwrap();
        assert!(matches!(err, ProxyError::Upstream { .. }), "{err:?}");
    }

    #[tokio::test]
    async fn bad_method_is_invalid() {
        let proxy = Proxy::new(Arc::new(StaticDirectory::default().with(
            "svc",
            Upstream::new(url("http://127.0.0.1:1/"), Placement::Local),
        )));
        let mut req = request("svc", "/");
        req.method = "GE T".into();
        assert!(matches!(
            proxy.send(req).await.err().unwrap(),
            ProxyError::InvalidRequest { .. }
        ));
    }
}
