//! The `mcp-app://` scheme: the MCP Apps sandbox proxy, on an origin of its own.
//!
//! An MCP App (a `ui://` resource, `text/html;profile=mcp-app`) is one HTML document
//! with inline scripts. A `srcdoc` frame in the shell would inherit the shell's CSP
//! (no inline scripts, `frame-src 'none'`), so the shell frames this proxy instead
//! (`mcp-app://localhost/sandbox.html`; `http://mcp-app.localhost/…` on Windows) and
//! the proxy frames the View (`assets/mcp-app/sandbox.js`). Its CSP comes as a
//! header (`assets/mcp-app/csp.txt`): no network, no IPC, frameable by the shell only.
//! docs/desktop.md, "MCP App sandbox", has the findings behind it.

use tauri::http::{Method, Request, Response, StatusCode, header};

pub const SCHEME: &str = "mcp-app";

const SANDBOX_HTML: &str = include_str!("../assets/mcp-app/sandbox.html");
const SANDBOX_JS: &str = include_str!("../assets/mcp-app/sandbox.js");
const SMOKE_PROBE_HTML: &str = include_str!("../assets/mcp-app/smoke-probe.html");
const CSP: &str = include_str!("../assets/mcp-app/csp.txt");
/// Where `sandbox.js` takes its parent-origin allowlist.
const ORIGINS_PLACEHOLDER: &str = "/*__MCP_APP_PARENT_ORIGINS__*/ []";

/// The shell's origins: the bundled app (`tauri://localhost`; `http://tauri.localhost`
/// on Windows), and in debug builds the Vite dev server.
pub fn parent_origins() -> Vec<&'static str> {
    let mut origins = vec!["tauri://localhost", "http://tauri.localhost"];
    if cfg!(debug_assertions) {
        origins.push("http://localhost:4323");
    }
    origins
}

/// Answers a request on the scheme. `smoke` also serves the smoke test's probe page.
pub fn respond(request: &Request<Vec<u8>>, origins: &[&str], smoke: bool) -> Response<Vec<u8>> {
    if request.method() != Method::GET {
        return status(StatusCode::METHOD_NOT_ALLOWED);
    }
    let (body, content_type) = match request.uri().path() {
        "/sandbox.html" => (SANDBOX_HTML.to_owned(), "text/html; charset=utf-8"),
        "/sandbox.js" => (
            SANDBOX_JS.replacen(
                ORIGINS_PLACEHOLDER,
                &serde_json::to_string(origins).expect("strings serialize"),
                1,
            ),
            "text/javascript; charset=utf-8",
        ),
        "/smoke-probe.html" if smoke => (SMOKE_PROBE_HTML.to_owned(), "text/html; charset=utf-8"),
        _ => return status(StatusCode::NOT_FOUND),
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_SECURITY_POLICY, csp(origins))
        .header(header::CACHE_CONTROL, "no-store")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff")
        .header(header::REFERRER_POLICY, "no-referrer")
        .body(body.into_bytes())
        .expect("a valid response")
}

/// The proxy's CSP, framed by `origins` only.
pub fn csp(origins: &[&str]) -> String {
    CSP.trim().replace("{parent_origins}", &origins.join(" "))
}

fn status(code: StatusCode) -> Response<Vec<u8>> {
    Response::builder()
        .status(code)
        .body(Vec::new())
        .expect("a valid response")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(path: &str, smoke: bool) -> Response<Vec<u8>> {
        let request = Request::builder()
            .uri(format!("mcp-app://localhost{path}"))
            .body(Vec::new())
            .unwrap();
        respond(
            &request,
            &["tauri://localhost", "http://tauri.localhost"],
            smoke,
        )
    }

    fn header(response: &Response<Vec<u8>>, name: header::HeaderName) -> &str {
        response.headers()[name].to_str().unwrap()
    }

    #[test]
    fn serves_the_proxy_with_its_csp() {
        let page = get("/sandbox.html", false);
        assert_eq!(page.status(), StatusCode::OK);
        assert!(header(&page, header::CONTENT_TYPE).starts_with("text/html"));
        let policy = header(&page, header::CONTENT_SECURITY_POLICY);
        assert!(policy.contains("connect-src 'none'"));
        assert!(policy.contains("script-src 'self' 'unsafe-inline'"));
        assert!(policy.ends_with("frame-ancestors tauri://localhost http://tauri.localhost"));
        assert!(String::from_utf8_lossy(page.body()).contains(r#"src="./sandbox.js""#));
    }

    #[test]
    fn fills_in_the_parent_origins() {
        let script = get("/sandbox.js", false);
        assert!(header(&script, header::CONTENT_TYPE).starts_with("text/javascript"));
        let body = String::from_utf8(script.body().clone()).unwrap();
        assert!(body.contains(
            r#"export const PARENT_ORIGINS = ["tauri://localhost","http://tauri.localhost"];"#
        ));
        assert!(!body.contains("__MCP_APP_PARENT_ORIGINS__"));
    }

    #[test]
    fn serves_the_probe_to_smoke_runs_only() {
        assert_eq!(
            get("/smoke-probe.html", false).status(),
            StatusCode::NOT_FOUND
        );
        let probe = get("/smoke-probe.html", true);
        assert_eq!(probe.status(), StatusCode::OK);
        assert!(header(&probe, header::CONTENT_SECURITY_POLICY).contains("connect-src 'none'"));
    }

    #[test]
    fn answers_anything_else_with_404_or_405() {
        for path in ["/", "/index.html", "/../tauri.conf.json", "/sandbox.html/x"] {
            assert_eq!(get(path, true).status(), StatusCode::NOT_FOUND, "{path}");
        }
        let post = Request::builder()
            .method(Method::POST)
            .uri("mcp-app://localhost/sandbox.html")
            .body(Vec::new())
            .unwrap();
        assert_eq!(
            respond(&post, &[], false).status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
    }

    #[test]
    fn debug_builds_allow_the_dev_server() {
        assert_eq!(
            parent_origins().contains(&"http://localhost:4323"),
            cfg!(debug_assertions)
        );
    }
}
