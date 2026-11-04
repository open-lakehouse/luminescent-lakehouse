//! [`CurrentDirectory`]: the services of the current environment, switched as it
//! changes (a running instance's gateway roots, or a Neon branch's functions),
//! under any bound by hand (`LUMINESCENT_SERVICES`), which win.

use std::sync::RwLock;

use reqwest::Url;

use crate::instances::ServiceRoot;
use crate::services::{Placement, ServiceDirectory, ServiceInfo, StaticDirectory, Upstream};

pub struct CurrentDirectory {
    overrides: StaticDirectory,
    current: RwLock<StaticDirectory>,
}

impl CurrentDirectory {
    /// Bound by hand: `overrides`, ahead of whatever the current environment binds.
    pub fn new(overrides: StaticDirectory) -> Self {
        CurrentDirectory {
            overrides,
            current: RwLock::new(StaticDirectory::default()),
        }
    }

    /// Bind the current environment's services, all at `placement`. Nothing changes
    /// when a root doesn't parse.
    pub fn set(&self, roots: &[ServiceRoot], placement: Placement) -> Result<(), String> {
        let mut dir = StaticDirectory::default();
        for root in roots {
            let url = Url::parse(&root.url).map_err(|e| format!("`{}`: {e}", root.url))?;
            dir = dir.with(&root.module_id, Upstream::new(url, placement));
        }
        *self.current.write().expect("directory lock") = dir;
        Ok(())
    }

    /// A running instance's gateway roots.
    pub fn set_local(&self, roots: &[ServiceRoot]) -> Result<(), String> {
        self.set(roots, Placement::Local)
    }

    /// A Neon branch's function URLs.
    pub fn set_remote(&self, roots: &[ServiceRoot]) -> Result<(), String> {
        self.set(roots, Placement::Remote)
    }

    /// No current environment: only what is bound by hand.
    pub fn clear(&self) {
        *self.current.write().expect("directory lock") = StaticDirectory::default();
    }
}

impl Default for CurrentDirectory {
    fn default() -> Self {
        CurrentDirectory::new(StaticDirectory::default())
    }
}

impl ServiceDirectory for CurrentDirectory {
    fn list(&self) -> Vec<ServiceInfo> {
        let mut list = self.overrides.list();
        for info in self.current.read().expect("directory lock").list() {
            if !list.iter().any(|i| i.module_id == info.module_id) {
                list.push(info);
            }
        }
        list.sort_by(|a, b| a.module_id.cmp(&b.module_id));
        list
    }

    fn resolve(&self, module_id: &str) -> Option<Upstream> {
        self.overrides.resolve(module_id).or_else(|| {
            self.current
                .read()
                .expect("directory lock")
                .resolve(module_id)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(module_id: &str, url: &str) -> ServiceRoot {
        ServiceRoot {
            module_id: module_id.into(),
            url: url.into(),
        }
    }

    #[test]
    fn follows_the_current_environment_under_the_overrides() {
        let overrides = StaticDirectory::parse("smoke-echo=http://127.0.0.1:1/").unwrap();
        let dir = CurrentDirectory::new(overrides);
        assert_eq!(dir.list().len(), 1);
        assert!(dir.resolve("unity-catalog").is_none());

        dir.set_local(&[
            root("unity-catalog", "http://localhost:9080/"),
            root("smoke-echo", "http://localhost:9999/"),
        ])
        .unwrap();
        let uc = dir.resolve("unity-catalog").unwrap();
        assert_eq!(uc.base_url.as_str(), "http://localhost:9080/");
        assert_eq!(uc.placement, Placement::Local);
        // Bound by hand wins.
        assert_eq!(
            dir.resolve("smoke-echo").unwrap().base_url.as_str(),
            "http://127.0.0.1:1/"
        );
        let ids: Vec<String> = dir.list().into_iter().map(|i| i.module_id).collect();
        assert_eq!(ids, ["smoke-echo", "unity-catalog"]);

        dir.clear();
        assert!(dir.resolve("unity-catalog").is_none());
        assert!(dir.resolve("smoke-echo").is_some());
    }

    #[test]
    fn a_neon_branch_binds_remote_roots() {
        let dir = CurrentDirectory::default();
        dir.set_local(&[root("headwaters", "http://localhost:9080/")])
            .unwrap();
        dir.set_remote(&[root(
            "unity-catalog",
            "https://br-x-unitycatalog.compute.neon.tech/",
        )])
        .unwrap();
        // Replaces what the instance bound.
        assert!(dir.resolve("headwaters").is_none());
        let uc = dir.resolve("unity-catalog").unwrap();
        assert_eq!(uc.placement, Placement::Remote);
        assert_eq!(dir.list()[0].placement, Placement::Remote);
    }

    #[test]
    fn a_bad_root_is_refused_and_changes_nothing() {
        let dir = CurrentDirectory::default();
        dir.set_local(&[root("a", "http://localhost:1/")]).unwrap();
        assert!(dir.set_local(&[root("b", "not a url")]).is_err());
        assert!(dir.resolve("a").is_some());
    }
}
