//! Generated message, view and enum types, one file per package (`just
//! factory gen`). This module tree is hand-written; the included files are
//! generated and never edited.
#![allow(
    dead_code,
    non_camel_case_types,
    unused_imports,
    clippy::derivable_impls,
    clippy::doc_lazy_continuation,
    clippy::match_single_binding,
    clippy::module_inception,
    clippy::uninlined_format_args
)]

pub mod factory {
    pub mod v1 {
        include!("factory.v1.rs");
    }
    pub mod workgraph {
        pub mod v1 {
            include!("factory.workgraph.v1.rs");
        }
    }
}
