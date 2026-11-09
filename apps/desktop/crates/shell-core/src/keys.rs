//! The keys the views address services by.
//!
//! A module that serves an API other implementations serve too (`unity-catalog-oss`
//! and `unity-catalog-rs` both serve `unity-catalog`) is addressed by that API, so the
//! views don't depend on which implementation an environment runs. Any other module
//! is addressed by its id. A function deployed on a Neon branch is addressed by the
//! key of the module it runs, never by its slug.

use std::collections::BTreeMap;

use olai_stack_topology::{Catalog, NeonRequirements};

/// Module ids and function slugs → the key the views address them by.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ServiceKeys {
    /// Module id → the API it serves, for the modules that declare one.
    apis: BTreeMap<String, String>,
    /// Function slug → the module it runs.
    slugs: BTreeMap<String, String>,
}

impl ServiceKeys {
    /// From the catalog a revision plans with, and its plan's Neon requirements
    /// (each function's slug and module).
    pub fn new(catalog: &Catalog, req: &NeonRequirements) -> Self {
        let apis = catalog
            .modules()
            .iter()
            .filter_map(|m| {
                let api = m.provides().api.clone()?;
                Some((m.id().as_str().to_owned(), api))
            })
            .collect();
        let slugs = req
            .functions
            .iter()
            .map(|f| (f.slug.clone(), f.module.as_str().to_owned()))
            .collect();
        ServiceKeys { apis, slugs }
    }

    /// The key of module `id`: the API it serves, else its id.
    pub fn module(&self, id: &str) -> String {
        self.apis.get(id).cloned().unwrap_or_else(|| id.to_owned())
    }

    /// The key of the function deployed as `slug`: its module's key, else the slug.
    pub fn function(&self, slug: &str) -> String {
        match self.slugs.get(slug) {
            Some(module) => self.module(module),
            None => slug.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use olai_stack_topology::{FunctionRequirement, ModuleId, baseline_catalog};

    use super::*;

    fn function(module: &str, slug: &str) -> FunctionRequirement {
        FunctionRequirement {
            module: ModuleId(module.into()),
            slug: slug.into(),
            url_var: None,
            env: Default::default(),
            secrets: Vec::new(),
        }
    }

    #[test]
    fn both_unity_catalogs_are_addressed_by_their_api() {
        let keys = ServiceKeys::new(&baseline_catalog(), &NeonRequirements::default());
        assert_eq!(keys.module("unity-catalog-oss"), "unity-catalog");
        assert_eq!(keys.module("unity-catalog-rs"), "unity-catalog");
        assert_eq!(keys.module("headwaters"), "headwaters");
        assert_eq!(keys.module("mlflow"), "mlflow");
    }

    #[test]
    fn a_function_is_addressed_by_its_modules_key() {
        let req = NeonRequirements {
            functions: vec![
                function("unity-catalog-rs", "unitycatalog"),
                function("headwaters", "headwaters"),
            ],
            ..NeonRequirements::default()
        };
        let keys = ServiceKeys::new(&baseline_catalog(), &req);
        assert_eq!(keys.function("unitycatalog"), "unity-catalog");
        assert_eq!(keys.function("headwaters"), "headwaters");
        // A slug the plan doesn't run keeps its name.
        assert_eq!(keys.function("opensharing"), "opensharing");
    }
}
