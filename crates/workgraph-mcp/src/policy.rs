//! Which tools a token may see and call.
//!
//! There are no built-in roles: what a planner or a reviewer is depends on
//! the workflow, which a composition defines. Whoever mints a token (the
//! composition, the reconciler from a profile or an agent's config) gives it
//! a [`ToolPolicy`] made of generic [`Selector`]s, and the server applies it
//! to every tool alike, built-in or an extension's.

use std::fmt;
use std::str::FromStr;

use factory_core::tool::ToolSpec;

/// Picks tools by name, by pattern, or by what the tool says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selector {
    /// Every tool (`*`).
    All,
    /// One tool by name (`plan_read`).
    Name(String),
    /// Tools whose names match a pattern with `*` wildcards (`plan_*`).
    Glob(String),
    /// Tools annotated read-only (`@read`).
    ReadOnly,
    /// Tools that may write (`@write`).
    Writes,
}

fn glob(pattern: &str, name: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(i) => rest = &rest[i + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

impl Selector {
    pub fn matches(&self, tool: &ToolSpec) -> bool {
        match self {
            Self::All => true,
            Self::Name(n) => *n == tool.name,
            Self::Glob(p) => glob(p, &tool.name),
            Self::ReadOnly => tool.read_only,
            Self::Writes => !tool.read_only,
        }
    }
}

impl FromStr for Selector {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        Ok(match s {
            "" => return Err("an empty tool selector".into()),
            "*" => Self::All,
            "@read" => Self::ReadOnly,
            "@write" => Self::Writes,
            s if s.starts_with('@') => {
                return Err(format!("unknown tool class {s:?}; use @read or @write"));
            }
            s if s.contains('*') => Self::Glob(s.into()),
            s => Self::Name(s.into()),
        })
    }
}

impl fmt::Display for Selector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::All => f.write_str("*"),
            Self::Name(s) | Self::Glob(s) => f.write_str(s),
            Self::ReadOnly => f.write_str("@read"),
            Self::Writes => f.write_str("@write"),
        }
    }
}

/// The tools a token may use: those an `allow` selector picks and no `deny`
/// selector does. An empty `allow` allows nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ToolPolicy {
    pub allow: Vec<Selector>,
    pub deny: Vec<Selector>,
}

impl ToolPolicy {
    /// Every tool.
    pub fn all() -> Self {
        Self::allow([Selector::All])
    }

    /// The read-only tools.
    pub fn read_only() -> Self {
        Self::allow([Selector::ReadOnly])
    }

    pub fn allow(selectors: impl IntoIterator<Item = Selector>) -> Self {
        Self {
            allow: selectors.into_iter().collect(),
            deny: Vec::new(),
        }
    }

    /// The tools named, exactly: an agent's own allowlist, mirrored.
    pub fn names<S: Into<String>>(names: impl IntoIterator<Item = S>) -> Self {
        Self::allow(names.into_iter().map(|n| Selector::Name(n.into())))
    }

    #[must_use]
    pub fn deny(mut self, selectors: impl IntoIterator<Item = Selector>) -> Self {
        self.deny.extend(selectors);
        self
    }

    pub fn allows(&self, tool: &ToolSpec) -> bool {
        self.allow.iter().any(|s| s.matches(tool)) && !self.deny.iter().any(|s| s.matches(tool))
    }
}

/// `sel,sel,!sel`: allowed selectors, and denied ones prefixed with `!`.
/// For example `@read,report_change`, or `*,!submit`.
impl FromStr for ToolPolicy {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut policy = Self::default();
        for part in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            match part.strip_prefix('!') {
                Some(denied) => policy.deny.push(denied.parse()?),
                None => policy.allow.push(part.parse()?),
            }
        }
        Ok(policy)
    }
}

impl fmt::Display for ToolPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts: Vec<String> = self
            .allow
            .iter()
            .map(ToString::to_string)
            .chain(self.deny.iter().map(|s| format!("!{s}")))
            .collect();
        f.write_str(&parts.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use factory_core::tool::{ToolCall, ToolHandler, ToolOutput};

    struct Nop;
    #[async_trait::async_trait]
    impl ToolHandler for Nop {
        async fn call(&self, _: ToolCall<'_>) -> factory_core::Result<ToolOutput> {
            Ok(ToolOutput::default())
        }
    }

    fn tool(name: &str, read_only: bool) -> ToolSpec {
        let t = ToolSpec::new(name, "", serde_json::json!({ "type": "object" }), Nop);
        if read_only { t.read_only() } else { t }
    }

    #[test]
    fn globs() {
        assert!(glob("plan_*", "plan_read"));
        assert!(glob("*_read", "item_read"));
        assert!(glob("a*b*c", "aXbYc"));
        assert!(!glob("a*b*c", "aXbY"));
        assert!(!glob("plan_*", "item_read"));
        assert!(glob("*", "anything"));
    }

    #[test]
    fn policies_select_and_deny() {
        let read = tool("plan_read", true);
        let write = tool("add_item", false);
        let submit = tool("submit", false);

        assert!(ToolPolicy::all().allows(&write));
        assert!(!ToolPolicy::default().allows(&read), "empty allows nothing");
        let ro = ToolPolicy::read_only();
        assert!(ro.allows(&read) && !ro.allows(&write));

        let p: ToolPolicy = "@read, add_item".parse().unwrap();
        assert!(p.allows(&read) && p.allows(&write) && !p.allows(&submit));

        let p: ToolPolicy = "*,!submit".parse().unwrap();
        assert!(p.allows(&write) && !p.allows(&submit));
        assert_eq!(p.to_string(), "*,!submit");

        let p: ToolPolicy = "@write,!add_*".parse().unwrap();
        assert!(p.allows(&submit) && !p.allows(&write) && !p.allows(&read));

        assert!("@nope".parse::<ToolPolicy>().is_err());
        assert!(ToolPolicy::names(["submit"]).allows(&submit));
    }
}
