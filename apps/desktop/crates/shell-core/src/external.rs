//! Links an embedded app asks the shell to open (an MCP App's `ui/open-link`): web
//! pages only, in the system's browser.

use reqwest::Url;

/// `link` as a URL the system browser may open: `http` or `https`, with a host and no
/// credentials. Anything else (`file:`, `javascript:`, a custom scheme that would start
/// another app) is refused with the reason.
pub fn external_url(link: &str) -> Result<Url, String> {
    let url = Url::parse(link.trim()).map_err(|e| format!("not a URL: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("only web links open, not `{}:`", url.scheme()));
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("a link needs a host".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("a link may not carry credentials".into());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_web_links_only() {
        for ok in [
            "https://docs.example.com/grants?x=1#y",
            "http://localhost:9080/",
            "  https://example.com  ",
        ] {
            assert!(external_url(ok).is_ok(), "{ok}");
        }
        for (bad, why) in [
            ("file:///etc/passwd", "file:"),
            ("javascript:alert(1)", "javascript:"),
            ("mcp-app://localhost/sandbox.html", "mcp-app:"),
            ("vscode://open", "vscode:"),
            ("https://user:pw@example.com/", "credentials"),
            ("not a url", "not a URL"),
        ] {
            let err = external_url(bad).unwrap_err();
            assert!(err.contains(why), "{bad}: {err}");
        }
    }
}
