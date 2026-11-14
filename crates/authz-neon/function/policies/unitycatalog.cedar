// Unity Catalog (mangrove's `UnityCatalog.cedarschema`): what the relations of
// `authz.grants` on its securables allow. Admins and bans are baseline.cedar.
//
// The resource is the securable by full name, with every ancestor as a parent
// (Metastore → Catalog → Schema → Table, Volume, RegisteredModel →
// ModelVersion, …). The loader puts the relations the principal holds on the
// resource *or an ancestor* into `principal.relations`, so a grant is inherited
// down the hierarchy: `select` on a catalog covers every table in it. Grants
// are keyed by full name, so they don't follow a rename.
//
// The relations, after Databricks' privileges (`luminescent-admin grant add`
// refuses any other on a `UnityCatalog::` object):
//
// | Relation         | Allows                                   |
// |------------------|------------------------------------------|
// | (any caller)     | read on the metastore itself (lists)     |
// | `browse`         | browse                                   |
// | `read`           | read, browse                             |
// | `use`            | use                                      |
// | `select`         | select, read, browse, use                |
// | `modify`         | write, read, browse, use                 |
// | `create`         | create, read, browse, use                |
// | `manage`         | manage, read, browse, use                |
// | `all_privileges` | every action but external_use            |
// | `external_use`   | external_use, and nothing else           |
//
// EXTERNAL USE only ever comes from an explicit `external_use` grant: never
// from `select`, `modify` or `all_privileges`, nor from being an admin. A
// credential vend asks for it in addition to `read` or `write`.

// Listing catalogs, credentials or external locations first asks `read` on
// the metastore, then filters each entry by `read` on it. So every caller may
// read the metastore itself (and nothing under it): one sees the catalogs one
// holds a grant on. Lists in a catalog or schema ask on that container, which a
// grant on it, or above it, covers.
@id("uc-list")
permit (
  principal,
  action == UnityCatalog::Action::"read",
  resource == UnityCatalog::Metastore::"metastore"
);

@id("uc-browse")
permit (principal, action == UnityCatalog::Action::"browse", resource)
when {
  principal.relations.containsAny(
    ["browse", "read", "select", "modify", "create", "manage", "all_privileges"]
  )
};

@id("uc-read")
permit (principal, action == UnityCatalog::Action::"read", resource)
when {
  principal.relations.containsAny(
    ["read", "select", "modify", "create", "manage", "all_privileges"]
  )
};

@id("uc-use")
permit (principal, action == UnityCatalog::Action::"use", resource)
when {
  principal.relations.containsAny(
    ["use", "select", "modify", "create", "manage", "all_privileges"]
  )
};

@id("uc-select")
permit (principal, action == UnityCatalog::Action::"select", resource)
when { principal.relations.containsAny(["select", "all_privileges"]) };

@id("uc-write")
permit (principal, action == UnityCatalog::Action::"write", resource)
when { principal.relations.containsAny(["modify", "all_privileges"]) };

@id("uc-create")
permit (principal, action == UnityCatalog::Action::"create", resource)
when { principal.relations.containsAny(["create", "all_privileges"]) };

@id("uc-manage")
permit (principal, action == UnityCatalog::Action::"manage", resource)
when { principal.relations.containsAny(["manage", "all_privileges"]) };

@id("uc-external-use")
permit (principal, action == UnityCatalog::Action::"external_use", resource)
when { principal.relations.contains("external_use") };
