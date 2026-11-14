// Unity Catalog (mangrove's `UnityCatalog.cedarschema`). The resource is the
// securable by full name, with every ancestor as a parent, so a grant on a
// catalog or schema covers what is in it: the loader puts the relations held on
// the resource or an ancestor into `principal.relations`.
//
// A first cut; #127 writes the full policies.

@id("uc-select")
permit (principal, action == UnityCatalog::Action::"select", resource)
when { principal.relations.contains("select") };

// EXTERNAL USE only from an explicit `external_use` grant: never from
// `select`, `read` or any other relation.
@id("uc-external-use")
permit (principal, action == UnityCatalog::Action::"external_use", resource)
when { principal.relations.contains("external_use") };
