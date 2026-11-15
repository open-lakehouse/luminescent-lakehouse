//! [`CurrentDirectory`]: the services of the current environment, switched as it
//! changes (a running instance's gateway roots, and the functions of the Neon branch
//! it runs on), under any bound by hand (`LUMINESCENT_SERVICES`), which win. Remote
//! ones carry their bearer token (`LUMINESCENT_SERVICE_TOKENS`).

use std::sync::RwLock;

use reqwest::Url;

use crate::instances::ServiceRoot;
use crate::services::{
    Placement, ServiceDirectory, ServiceInfo, ServiceTokens, StaticDirectory, Upstream,
};

pub struct CurrentDirectory {
    overrides: StaticDirectory,
    current: RwLock<StaticDirectory>,
    tokens: ServiceTokens,
}

impl CurrentDirectory {
    /// Bound by hand: `overrides`, ahead of whatever the current environment binds.
    pub fn new(overrides: StaticDirectory) -> Self {
        CurrentDirectory {
            overrides,
            current: RwLock::new(StaticDirectory::default()),
            tokens: ServiceTokens::default(),
        }
    }

    /// Send `tokens` to the remote upstreams of their modules, bound by hand or not.
    /// A function the gateway fronts is remote too, so the gateway gets its token to
    /// forward.
    pub fn with_tokens(mut self, tokens: ServiceTokens) -> Self {
        self.tokens = tokens;
        self
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

    /// A running instance: its gateway roots (`local`) and the function URLs of the
    /// Neon branch it runs on (`remote`), as [`instance_directory`] binds them.
    pub fn set_instance(
        &self,
        local: &[ServiceRoot],
        remote: &[ServiceRoot],
    ) -> Result<(), String> {
        let dir = instance_directory(local, remote)?;
        *self.current.write().expect("directory lock") = dir;
        Ok(())
    }

    /// Where `module_id` of an instance with these roots is reached, current or not:
    /// what [`ServiceDirectory::resolve`] would give were it current (bound by hand
    /// first, its token on a remote upstream). For the host's own clients of a
    /// service, such as the query engine, which may target an instance that isn't
    /// current.
    pub fn resolve_instance(
        &self,
        local: &[ServiceRoot],
        remote: &[ServiceRoot],
        module_id: &str,
    ) -> Result<Option<Upstream>, String> {
        let upstream = match self.overrides.resolve(module_id) {
            Some(upstream) => Some(upstream),
            None => instance_directory(local, remote)?.resolve(module_id),
        };
        Ok(upstream.map(|upstream| self.tokens.apply(module_id, upstream)))
    }

    /// No current environment: only what is bound by hand.
    pub fn clear(&self) {
        *self.current.write().expect("directory lock") = StaticDirectory::default();
    }
}

/// The services of a running instance: its gateway roots (`local`) and the function
/// URLs of the Neon branch it runs on (`remote`). A function the gateway fronts is
/// reached there, at its gateway root, and still shows as `remote`; one it doesn't
/// front is reached at its URL. Refused when a root doesn't parse.
pub fn instance_directory(
    local: &[ServiceRoot],
    remote: &[ServiceRoot],
) -> Result<StaticDirectory, String> {
    let parse =
        |root: &ServiceRoot| Url::parse(&root.url).map_err(|e| format!("`{}`: {e}", root.url));
    let mut dir = StaticDirectory::default();
    for root in local {
        if remote.iter().any(|r| r.module_id == root.module_id) {
            continue;
        }
        dir = dir.with(
            &root.module_id,
            Upstream::new(parse(root)?, Placement::Local),
        );
    }
    for root in remote {
        let fronted = local.iter().find(|l| l.module_id == root.module_id);
        let url = parse(fronted.unwrap_or(root))?;
        dir = dir.with(&root.module_id, Upstream::new(url, Placement::Remote));
    }
    Ok(dir)
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
        let upstream = self.overrides.resolve(module_id).or_else(|| {
            self.current
                .read()
                .expect("directory lock")
                .resolve(module_id)
        })?;
        Some(self.tokens.apply(module_id, upstream))
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
    fn remote_upstreams_carry_their_module_s_token() {
        let overrides =
            StaticDirectory::parse("headwaters=https://br-x-headwaters.compute.neon.tech/")
                .unwrap();
        let tokens =
            ServiceTokens::parse("unity-catalog=olk_uc,headwaters=olk_hw,rustfs=olk_fs").unwrap();
        let dir = CurrentDirectory::new(overrides).with_tokens(tokens);
        dir.set_instance(
            &[root("rustfs", "http://localhost:9080/")],
            &[root(
                "unity-catalog",
                "https://br-x-unitycatalog.compute.neon.tech/",
            )],
        )
        .unwrap();
        let bearer = |module: &str| {
            dir.resolve(module)
                .unwrap()
                .headers
                .into_iter()
                .find(|(n, _)| n == "authorization")
                .map(|(_, v)| v)
        };
        assert_eq!(bearer("unity-catalog").as_deref(), Some("Bearer olk_uc"));
        // Bound by hand, on a branch.
        assert_eq!(bearer("headwaters").as_deref(), Some("Bearer olk_hw"));
        // Local modules never get one.
        assert_eq!(bearer("rustfs"), None);
    }

    #[test]
    fn an_instance_on_a_branch_binds_its_gateway_and_the_branch_s_functions() {
        let dir = CurrentDirectory::default();
        dir.set_instance(
            &[root("rustfs", "http://localhost:9080/")],
            &[root(
                "unity-catalog",
                "https://br-x-unitycatalog.compute.neon.tech/",
            )],
        )
        .unwrap();
        assert_eq!(dir.resolve("rustfs").unwrap().placement, Placement::Local);
        let uc = dir.resolve("unity-catalog").unwrap();
        assert_eq!(uc.placement, Placement::Remote);
        assert_eq!(
            uc.base_url.as_str(),
            "https://br-x-unitycatalog.compute.neon.tech/"
        );
        // A function the gateway fronts is reached there, still on the branch.
        dir.set_instance(
            &[
                root("rustfs", "http://localhost:9080/"),
                root("headwaters", "http://localhost:9080/lineage/"),
            ],
            &[root(
                "headwaters",
                "https://br-x-headwaters.compute.neon.tech/",
            )],
        )
        .unwrap();
        let hw = dir.resolve("headwaters").unwrap();
        assert_eq!(hw.placement, Placement::Remote);
        assert_eq!(hw.base_url.as_str(), "http://localhost:9080/lineage/");
        // The next instance replaces both.
        dir.set_instance(&[root("headwaters", "http://localhost:10080/")], &[])
            .unwrap();
        assert!(dir.resolve("unity-catalog").is_none());
        assert_eq!(dir.list().len(), 1);
    }

    #[test]
    fn resolves_an_instance_that_is_not_current() {
        let tokens = ServiceTokens::parse("unity-catalog=olk_uc").unwrap();
        let dir = CurrentDirectory::default().with_tokens(tokens);
        dir.set_local(&[root("unity-catalog", "http://localhost:9080/")])
            .unwrap();
        let bearer = |up: &Upstream| {
            up.headers
                .iter()
                .find(|(n, _)| n == "authorization")
                .map(|(_, v)| v.clone())
        };

        // Another instance, local: its own root, no token.
        let local = dir
            .resolve_instance(
                &[root("unity-catalog", "http://localhost:10080/")],
                &[],
                "unity-catalog",
            )
            .unwrap()
            .unwrap();
        assert_eq!(local.base_url.as_str(), "http://localhost:10080/");
        assert_eq!(local.placement, Placement::Local);
        assert_eq!(bearer(&local), None);

        // On a branch: the function, with its token.
        let remote = dir
            .resolve_instance(
                &[root("rustfs", "http://localhost:10080/")],
                &[root(
                    "unity-catalog",
                    "https://br-x-unitycatalog.compute.neon.tech/",
                )],
                "unity-catalog",
            )
            .unwrap()
            .unwrap();
        assert_eq!(remote.placement, Placement::Remote);
        assert_eq!(bearer(&remote).as_deref(), Some("Bearer olk_uc"));

        // Fronted by the gateway: reached there, still remote, still with the token.
        let fronted = dir
            .resolve_instance(
                &[root("unity-catalog", "http://localhost:10080/uc/")],
                &[root(
                    "unity-catalog",
                    "https://br-x-unitycatalog.compute.neon.tech/",
                )],
                "unity-catalog",
            )
            .unwrap()
            .unwrap();
        assert_eq!(fronted.base_url.as_str(), "http://localhost:10080/uc/");
        assert_eq!(bearer(&fronted).as_deref(), Some("Bearer olk_uc"));

        // Neither is bound now: the current one is untouched.
        assert_eq!(
            dir.resolve("unity-catalog").unwrap().base_url.as_str(),
            "http://localhost:9080/"
        );
        assert!(
            dir.resolve_instance(&[], &[], "unity-catalog")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn bound_by_hand_wins_for_any_instance() {
        let overrides =
            StaticDirectory::parse("unity-catalog=https://br-y-unitycatalog.compute.neon.tech/")
                .unwrap();
        let dir = CurrentDirectory::new(overrides);
        let uc = dir
            .resolve_instance(
                &[root("unity-catalog", "http://localhost:9080/")],
                &[],
                "unity-catalog",
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            uc.base_url.as_str(),
            "https://br-y-unitycatalog.compute.neon.tech/"
        );
    }

    #[test]
    fn a_bad_root_is_refused_and_changes_nothing() {
        let dir = CurrentDirectory::default();
        dir.set_local(&[root("a", "http://localhost:1/")]).unwrap();
        assert!(dir.set_local(&[root("b", "not a url")]).is_err());
        assert!(dir.resolve("a").is_some());
    }
}
