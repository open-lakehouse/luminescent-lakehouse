// Unity Catalog (mangrove's `UnityCatalog.cedarschema`, the generated actions
// and `DeltaApi.cedarschema`): what the grants of `authz.grants` allow, after
// Databricks' privilege model. Admins and bans are baseline.cedar.
//
// Every request is one operation (the action, an RPC of the API) on one
// securable: a list on its container, a create on the new securable, anything
// else on the securable itself. The server sends the securable and its
// ancestors (Metastore → Catalog → Schema → Table, Volume, …), each with typed
// attributes for its containers (`resource.schema`, `resource.catalog`). The
// loader puts on each of them the privileges the principal holds, itself or
// through a group:
//
// - `privileges`: granted on that securable itself, plus `OWNER` if it owns it;
// - `inherited_privileges`: on it or any ancestor (SELECT on a catalog covers
//   its tables).
//
// | Operation                               | Needs                                                        |
// |-----------------------------------------|--------------------------------------------------------------|
// | anything in a catalog / schema          | usage: USE_CATALOG on the catalog, USE_SCHEMA on the schema  |
// | List* on the metastore                  | nothing; each item is then filtered by its Get*              |
// | List* in a catalog / schema             | usage of the container, or BROWSE                            |
// | Get* (metadata)                         | usage + a privilege on it (not a USE_* or a CREATE_*), or BROWSE |
// | CreateCatalog, Create<metastore thing>  | CREATE_CATALOG, CREATE_STORAGE_CREDENTIAL, CREATE_EXTERNAL_LOCATION, CREATE_SHARE, CREATE_RECIPIENT, CREATE_PROVIDER; MANAGE for tag policies |
// | CreateSchema                            | usage + CREATE_SCHEMA on the catalog                         |
// | Create<table, volume, function, model>  | usage + CREATE_TABLE, CREATE_VOLUME, CREATE_FUNCTION, CREATE_MODEL on the schema (agents: CREATE_MODEL, agent skills: CREATE_FUNCTION) |
// | CreateModelVersion                      | usage + CREATE_MODEL_VERSION, or owning the model            |
// | Update*, Delete*, RenameTable, …        | usage + MANAGE, or owning it or a container                  |
// | CommitTable                             | usage + SELECT + MODIFY                                      |
// | GenerateTemporaryTableCredentials       | usage + SELECT (READ) / + MODIFY (READ_WRITE) + EXTERNAL_USE_SCHEMA on the schema |
// | GenerateTemporaryVolumeCredentials      | usage + READ_VOLUME / + WRITE_VOLUME + EXTERNAL_USE_SCHEMA   |
// | GenerateTemporaryModelVersionCredentials| usage + EXECUTE (READ) / MANAGE or owning (READ_WRITE) + EXTERNAL_USE_SCHEMA |
// | GenerateTemporaryPathCredentials        | READ_FILES / + WRITE_FILES + EXTERNAL_USE_LOCATION on the location |
// | CreateStagingTable                      | usage + CREATE_TABLE + EXTERNAL_USE_SCHEMA                   |
// | GenerateTemporaryStagingTableCredentials| usage + EXTERNAL_USE_SCHEMA                                  |
// | AdoptStagingTable                       | the reservation's creator + usage + CREATE_TABLE             |
// | GetGrants                               | MANAGE, READ_METADATA, owning it or a container, or a privilege on it |
// | UpdateGrants                            | usage + MANAGE, or owning it or a container                  |
// | entity tags: get, list / change         | metadata access / usage + APPLY_TAG, MANAGE or owning        |
// | ABAC policies: get, list / change       | READ_METADATA, BROWSE, MANAGE or owning / MANAGE or owning   |
// | share permissions: get / update         | MANAGE, SET_SHARE_PERMISSION or owning the share             |
//
// ALL_PRIVILEGES stands for every privilege a grant names here except
// EXTERNAL_USE_SCHEMA, EXTERNAL_USE_LOCATION and MANAGE, as in Databricks: it
// never hands out storage credentials nor the right to change grants. Owning a
// securable is every privilege on it but EXTERNAL USE; owning a container lets
// one manage and use what is in it, but not read its data.
//
// Grants are keyed by full name; the server re-keys them on a rename.

// Lists of the metastore's securables: every caller may ask; the server drops
// the entries the caller can't see (their Get*).
@id("uc-list-metastore")
permit (
  principal,
  action in [
    UnityCatalog::Action::"ListCatalogs",
    UnityCatalog::Action::"ListCredentials",
    UnityCatalog::Action::"ListExternalLocations",
    UnityCatalog::Action::"ListShares",
    UnityCatalog::Action::"ListRecipients",
    UnityCatalog::Action::"ListProviders",
    UnityCatalog::Action::"ListTagPolicies"
  ],
  resource
);

// Lists in a catalog.
@id("uc-list-catalog")
permit (
  principal,
  action in [
    UnityCatalog::Action::"ListSchemas",
    UnityCatalog::Action::"ListTableSummaries"
  ],
  resource
)
when {
  resource.inherited_privileges.containsAny(
    ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER", "BROWSE"]
  )
};

// Lists in a schema.
@id("uc-list-schema")
permit (
  principal,
  action in [
    UnityCatalog::Action::"ListTables",
    UnityCatalog::Action::"ListVolumes",
    UnityCatalog::Action::"ListFunctions",
    UnityCatalog::Action::"ListRegisteredModels",
    UnityCatalog::Action::"ListAgents",
    UnityCatalog::Action::"ListAgentSkills"
  ],
  resource
)
when {
  (resource.catalog.inherited_privileges.containsAny(
     ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
   ) &&
   resource.inherited_privileges.containsAny(
     ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
   )) ||
  resource.inherited_privileges.contains("BROWSE")
};

// The metadata of what is in a schema (and a registered model's versions).
@id("uc-metadata")
permit (
  principal,
  action in [
    UnityCatalog::Action::"GetTable",
    UnityCatalog::Action::"GetTableExists",
    UnityCatalog::Action::"GetVolume",
    UnityCatalog::Action::"GetFunction",
    UnityCatalog::Action::"GetRegisteredModel",
    UnityCatalog::Action::"ListModelVersions",
    UnityCatalog::Action::"GetModelVersion",
    UnityCatalog::Action::"GetAgent",
    UnityCatalog::Action::"GetAgentSkill"
  ],
  resource
)
when {
  (resource.catalog.inherited_privileges.containsAny(
     ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
   ) &&
   resource.schema.inherited_privileges.containsAny(
     ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
   ) &&
   resource.inherited_privileges.containsAny(
     [
       "SELECT",
       "MODIFY",
       "EXECUTE",
       "READ_VOLUME",
       "WRITE_VOLUME",
       "CREATE_MODEL_VERSION",
       "APPLY_TAG",
       "READ_METADATA",
       "MANAGE",
       "ALL_PRIVILEGES",
       "OWNER"
     ]
   )) ||
  resource.inherited_privileges.contains("BROWSE")
};

// A schema's metadata.
@id("uc-metadata-schema")
permit (principal, action == UnityCatalog::Action::"GetSchema", resource)
when {
  (resource.catalog.inherited_privileges.containsAny(
     ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
   ) &&
   resource.inherited_privileges.containsAny(
     [
       "USE_SCHEMA",
       "CREATE_TABLE",
       "CREATE_VOLUME",
       "CREATE_FUNCTION",
       "CREATE_MODEL",
       "CREATE_MANAGED_STORAGE",
       "EXTERNAL_USE_SCHEMA",
       "SELECT",
       "MODIFY",
       "EXECUTE",
       "READ_VOLUME",
       "WRITE_VOLUME",
       "APPLY_TAG",
       "READ_METADATA",
       "MANAGE",
       "ALL_PRIVILEGES",
       "OWNER"
     ]
   )) ||
  resource.inherited_privileges.contains("BROWSE")
};

// A catalog's metadata.
@id("uc-metadata-catalog")
permit (principal, action == UnityCatalog::Action::"GetCatalog", resource)
when {
  resource.inherited_privileges.containsAny(
    [
      "USE_CATALOG",
      "CREATE_SCHEMA",
      "USE_SCHEMA",
      "CREATE_TABLE",
      "CREATE_VOLUME",
      "CREATE_FUNCTION",
      "CREATE_MODEL",
      "CREATE_MANAGED_STORAGE",
      "EXTERNAL_USE_SCHEMA",
      "SELECT",
      "MODIFY",
      "EXECUTE",
      "READ_VOLUME",
      "WRITE_VOLUME",
      "APPLY_TAG",
      "READ_METADATA",
      "BROWSE",
      "MANAGE",
      "ALL_PRIVILEGES",
      "OWNER"
    ]
  )
};

// The metadata of the metastore's securables: any privilege on it, or one on
// the metastore that sees them all.
@id("uc-metadata-metastore-securable")
permit (
  principal,
  action in [
    UnityCatalog::Action::"GetCredential",
    UnityCatalog::Action::"GetExternalLocation",
    UnityCatalog::Action::"GetShare",
    UnityCatalog::Action::"GetRecipient",
    UnityCatalog::Action::"GetProvider",
    UnityCatalog::Action::"GetTagPolicy"
  ],
  resource
)
when {
  !resource.privileges.isEmpty() ||
  resource.inherited_privileges.containsAny(
    [
      "USE_SHARE",
      "USE_RECIPIENT",
      "USE_PROVIDER",
      "READ_METADATA",
      "BROWSE",
      "MANAGE",
      "OWNER"
    ]
  )
};

// Creating a catalog, or one of the metastore's securables: the CREATE_*
// privilege, granted on the metastore.
@id("uc-create-metastore-securable")
permit (
  principal,
  action in [
    UnityCatalog::Action::"CreateCatalog",
    UnityCatalog::Action::"CreateCredential",
    UnityCatalog::Action::"CreateExternalLocation",
    UnityCatalog::Action::"CreateShare",
    UnityCatalog::Action::"CreateRecipient",
    UnityCatalog::Action::"CreateProvider",
    UnityCatalog::Action::"CreateTagPolicy"
  ],
  resource
)
when {
  resource.inherited_privileges.containsAny(
    if action == UnityCatalog::Action::"CreateCatalog"
    then ["CREATE_CATALOG", "ALL_PRIVILEGES"]
    else if action == UnityCatalog::Action::"CreateCredential"
    then ["CREATE_STORAGE_CREDENTIAL", "ALL_PRIVILEGES"]
    else if action == UnityCatalog::Action::"CreateExternalLocation"
    then ["CREATE_EXTERNAL_LOCATION", "ALL_PRIVILEGES"]
    else if action == UnityCatalog::Action::"CreateShare"
    then ["CREATE_SHARE", "ALL_PRIVILEGES"]
    else if action == UnityCatalog::Action::"CreateRecipient"
    then ["CREATE_RECIPIENT", "ALL_PRIVILEGES"]
    else if action == UnityCatalog::Action::"CreateProvider"
    then ["CREATE_PROVIDER", "ALL_PRIVILEGES"]
    else ["MANAGE"]
  )
};

// Creating a schema: usage of the catalog and CREATE_SCHEMA on it.
@id("uc-create-schema")
permit (principal, action == UnityCatalog::Action::"CreateSchema", resource)
when {
  resource.catalog.inherited_privileges.containsAny(
    ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.catalog.inherited_privileges.containsAny(
    ["CREATE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
  )
};

// Creating in a schema: usage, and the CREATE_* privilege on the schema.
@id("uc-create-in-schema")
permit (
  principal,
  action in [
    UnityCatalog::Action::"CreateTable",
    UnityCatalog::Action::"CreateVolume",
    UnityCatalog::Action::"CreateFunction",
    UnityCatalog::Action::"CreateRegisteredModel",
    UnityCatalog::Action::"CreateAgent",
    UnityCatalog::Action::"CreateAgentSkill"
  ],
  resource
)
when {
  resource.catalog.inherited_privileges.containsAny(
    ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.containsAny(
    ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.containsAny(
    if action == UnityCatalog::Action::"CreateTable"
    then ["CREATE_TABLE", "ALL_PRIVILEGES", "OWNER"]
    else if action == UnityCatalog::Action::"CreateVolume"
    then ["CREATE_VOLUME", "ALL_PRIVILEGES", "OWNER"]
    else if action == UnityCatalog::Action::"CreateFunction" ||
            action == UnityCatalog::Action::"CreateAgentSkill"
    then ["CREATE_FUNCTION", "ALL_PRIVILEGES", "OWNER"]
    else ["CREATE_MODEL", "ALL_PRIVILEGES", "OWNER"]
  )
};

// A new version of a registered model (asked on the model).
@id("uc-create-model-version")
permit (
  principal,
  action == UnityCatalog::Action::"CreateModelVersion",
  resource
)
when {
  resource.catalog.inherited_privileges.containsAny(
    ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.containsAny(
    ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  (resource.inherited_privileges.containsAny(
     ["CREATE_MODEL_VERSION", "ALL_PRIVILEGES"]
   ) ||
   resource.privileges.contains("OWNER"))
};

// Changing or deleting what is in a schema: usage, and MANAGE on it or owning
// it or a container.
@id("uc-manage")
permit (
  principal,
  action in [
    UnityCatalog::Action::"DeleteTable",
    UnityCatalog::Action::"RenameTable",
    UnityCatalog::Action::"UpdateVolume",
    UnityCatalog::Action::"DeleteVolume",
    UnityCatalog::Action::"UpdateFunction",
    UnityCatalog::Action::"DeleteFunction",
    UnityCatalog::Action::"UpdateRegisteredModel",
    UnityCatalog::Action::"DeleteRegisteredModel",
    UnityCatalog::Action::"UpdateModelVersion",
    UnityCatalog::Action::"DeleteModelVersion",
    UnityCatalog::Action::"FinalizeModelVersion",
    UnityCatalog::Action::"UpdateAgent",
    UnityCatalog::Action::"DeleteAgent",
    UnityCatalog::Action::"UpdateAgentSkill",
    UnityCatalog::Action::"DeleteAgentSkill"
  ],
  resource
)
when {
  resource.catalog.inherited_privileges.containsAny(
    ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.containsAny(
    ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.inherited_privileges.containsAny(["MANAGE", "OWNER"])
};

// Changing or deleting a schema: usage of its catalog, and MANAGE or owning.
@id("uc-manage-schema")
permit (
  principal,
  action in [
    UnityCatalog::Action::"UpdateSchema",
    UnityCatalog::Action::"DeleteSchema"
  ],
  resource
)
when {
  resource.catalog.inherited_privileges.containsAny(
    ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.inherited_privileges.containsAny(["MANAGE", "OWNER"])
};

// Changing or deleting a catalog or a metastore securable: MANAGE or owning.
@id("uc-manage-metastore-securable")
permit (
  principal,
  action in [
    UnityCatalog::Action::"UpdateCatalog",
    UnityCatalog::Action::"DeleteCatalog",
    UnityCatalog::Action::"UpdateCredential",
    UnityCatalog::Action::"DeleteCredential",
    UnityCatalog::Action::"UpdateExternalLocation",
    UnityCatalog::Action::"DeleteExternalLocation",
    UnityCatalog::Action::"UpdateShare",
    UnityCatalog::Action::"DeleteShare",
    UnityCatalog::Action::"UpdateRecipient",
    UnityCatalog::Action::"DeleteRecipient",
    UnityCatalog::Action::"RotateRecipientToken",
    UnityCatalog::Action::"UpdateProvider",
    UnityCatalog::Action::"DeleteProvider",
    UnityCatalog::Action::"UpdateTagPolicy",
    UnityCatalog::Action::"DeleteTagPolicy"
  ],
  resource
)
when { resource.inherited_privileges.containsAny(["MANAGE", "OWNER"]) };

// Committing to a table (the Delta API): usage, SELECT and MODIFY.
@id("uc-commit")
permit (principal, action == UnityCatalog::Action::"CommitTable", resource)
when {
  resource.catalog.inherited_privileges.containsAny(
    ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.containsAny(
    ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  (resource.inherited_privileges.containsAny(["SELECT", "ALL_PRIVILEGES"]) ||
   resource.privileges.contains("OWNER")) &&
  (resource.inherited_privileges.containsAny(["MODIFY", "ALL_PRIVILEGES"]) ||
   resource.privileges.contains("OWNER"))
};

// Credentials for a table, a volume or a model version: usage, the data
// privilege of the operation asked (`context.credential_operation`), and
// EXTERNAL_USE_SCHEMA on the schema.
@id("uc-vend")
permit (
  principal,
  action in [
    UnityCatalog::Action::"GenerateTemporaryTableCredentials",
    UnityCatalog::Action::"GenerateTemporaryVolumeCredentials",
    UnityCatalog::Action::"GenerateTemporaryModelVersionCredentials"
  ],
  resource
)
when {
  context has credential_operation &&
  resource.catalog.inherited_privileges.containsAny(
    ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.containsAny(
    ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.contains("EXTERNAL_USE_SCHEMA") &&
  (resource.inherited_privileges.containsAny(
     if action == UnityCatalog::Action::"GenerateTemporaryTableCredentials"
     then ["SELECT", "ALL_PRIVILEGES"]
     else if action == UnityCatalog::Action::"GenerateTemporaryVolumeCredentials"
     then ["READ_VOLUME", "ALL_PRIVILEGES"]
     else ["EXECUTE", "ALL_PRIVILEGES"]
   ) ||
   resource.privileges.contains("OWNER")) &&
  (context.credential_operation == "READ" ||
   (context.credential_operation == "READ_WRITE" &&
    (resource.inherited_privileges.containsAny(
       if action == UnityCatalog::Action::"GenerateTemporaryTableCredentials"
       then ["MODIFY", "ALL_PRIVILEGES"]
       else if action == UnityCatalog::Action::"GenerateTemporaryVolumeCredentials"
       then ["WRITE_VOLUME", "ALL_PRIVILEGES"]
       else ["MANAGE"]
     ) ||
     resource.privileges.contains("OWNER"))))
};

// Credentials for a path, asked on the external location covering it:
// READ_FILES (and WRITE_FILES to write) and EXTERNAL_USE_LOCATION on it.
@id("uc-vend-path")
permit (
  principal,
  action == UnityCatalog::Action::"GenerateTemporaryPathCredentials",
  resource
)
when {
  context has credential_operation &&
  resource.inherited_privileges.contains("EXTERNAL_USE_LOCATION") &&
  (resource.inherited_privileges.containsAny(["READ_FILES", "ALL_PRIVILEGES"]) ||
   resource.privileges.contains("OWNER")) &&
  (context.credential_operation == "READ" ||
   (context.credential_operation == "READ_WRITE" &&
    (resource.inherited_privileges.containsAny(
       ["WRITE_FILES", "ALL_PRIVILEGES"]
     ) ||
     resource.privileges.contains("OWNER"))))
};

// Reserving a staging table (and its location) for a new managed table:
// usage, CREATE_TABLE and EXTERNAL_USE_SCHEMA on the schema.
@id("uc-staging-create")
permit (
  principal,
  action == UnityCatalog::Action::"CreateStagingTable",
  resource
)
when {
  resource.catalog.inherited_privileges.containsAny(
    ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.containsAny(
    ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.containsAny(
    ["CREATE_TABLE", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.contains("EXTERNAL_USE_SCHEMA")
};

// Credentials for a staging table's location: usage and EXTERNAL_USE_SCHEMA.
@id("uc-staging-vend")
permit (
  principal,
  action == UnityCatalog::Action::"GenerateTemporaryStagingTableCredentials",
  resource
)
when {
  resource.catalog.inherited_privileges.containsAny(
    ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.containsAny(
    ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.contains("EXTERNAL_USE_SCHEMA")
};

// Turning a staging table into a managed table: only who reserved it, with
// usage and CREATE_TABLE on the schema.
@id("uc-staging-adopt")
permit (
  principal,
  action == UnityCatalog::Action::"AdoptStagingTable",
  resource
)
when {
  context has is_creator &&
  context.is_creator &&
  resource.catalog.inherited_privileges.containsAny(
    ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.containsAny(
    ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
  ) &&
  resource.schema.inherited_privileges.containsAny(
    ["CREATE_TABLE", "ALL_PRIVILEGES", "OWNER"]
  )
};

// An admin's vends: baseline.cedar's `admin` leaves them out, so an admin needs
// the EXTERNAL USE grant like anyone else, but no data privilege.
@id("uc-vend-admin")
permit (
  principal is Lakehouse::User,
  action in [
    UnityCatalog::Action::"GenerateTemporaryTableCredentials",
    UnityCatalog::Action::"GenerateTemporaryVolumeCredentials",
    UnityCatalog::Action::"GenerateTemporaryModelVersionCredentials",
    UnityCatalog::Action::"GenerateTemporaryPathCredentials",
    UnityCatalog::Action::"GenerateTemporaryStagingTableCredentials",
    UnityCatalog::Action::"CreateStagingTable"
  ],
  resource
)
when {
  principal.neon_role == "admin" &&
  (if resource has schema
   then resource.schema.inherited_privileges.contains("EXTERNAL_USE_SCHEMA")
   else resource.inherited_privileges.contains("EXTERNAL_USE_LOCATION"))
};

// Reading a securable's grants (any securable): MANAGE, READ_METADATA, owning
// it or a container, or holding a privilege on it, with usage of its
// containers.
@id("uc-grants-read")
permit (principal, action == UnityCatalog::Action::"GetGrants", resource)
when {
  (!(resource has catalog) ||
   resource.catalog.inherited_privileges.containsAny(
     ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
   )) &&
  (!(resource has schema) ||
   resource.schema.inherited_privileges.containsAny(
     ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
   )) &&
  (!resource.privileges.isEmpty() ||
   resource.inherited_privileges.containsAny(
     ["READ_METADATA", "MANAGE", "OWNER"]
   ))
};

// Changing a securable's grants: MANAGE on it or owning it or a container,
// with usage of its containers.
@id("uc-grants-update")
permit (principal, action == UnityCatalog::Action::"UpdateGrants", resource)
when {
  (!(resource has catalog) ||
   resource.catalog.inherited_privileges.containsAny(
     ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
   )) &&
  (!(resource has schema) ||
   resource.schema.inherited_privileges.containsAny(
     ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
   )) &&
  resource.inherited_privileges.containsAny(["MANAGE", "OWNER"])
};

// A securable's tags: seeing them takes metadata access (a privilege on it,
// READ_METADATA, BROWSE, MANAGE or owning).
@id("uc-tags-read")
permit (
  principal,
  action in [
    UnityCatalog::Action::"ListEntityTagAssignments",
    UnityCatalog::Action::"GetEntityTagAssignment"
  ],
  resource
)
when {
  resource.inherited_privileges.contains("BROWSE") ||
  ((!(resource has catalog) ||
    resource.catalog.inherited_privileges.containsAny(
      ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
    )) &&
   (!(resource has schema) ||
    resource.schema.inherited_privileges.containsAny(
      ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
    )) &&
   (!resource.privileges.isEmpty() ||
    resource.inherited_privileges.containsAny(
      ["READ_METADATA", "MANAGE", "OWNER"]
    )))
};

// Tagging a securable: usage, and APPLY_TAG, MANAGE or owning it.
@id("uc-tags-change")
permit (
  principal,
  action in [
    UnityCatalog::Action::"CreateEntityTagAssignment",
    UnityCatalog::Action::"UpdateEntityTagAssignment",
    UnityCatalog::Action::"DeleteEntityTagAssignment"
  ],
  resource
)
when {
  (!(resource has catalog) ||
   resource.catalog.inherited_privileges.containsAny(
     ["USE_CATALOG", "ALL_PRIVILEGES", "OWNER"]
   )) &&
  (!(resource has schema) ||
   resource.schema.inherited_privileges.containsAny(
     ["USE_SCHEMA", "ALL_PRIVILEGES", "OWNER"]
   )) &&
  resource.inherited_privileges.containsAny(
    ["APPLY_TAG", "ALL_PRIVILEGES", "MANAGE", "OWNER"]
  )
};

// The ABAC policies on a securable (row filters, column masks): reading them
// takes READ_METADATA, BROWSE, MANAGE or owning it or a container.
@id("uc-policies-read")
permit (
  principal,
  action in [
    UnityCatalog::Action::"ListPolicies",
    UnityCatalog::Action::"GetPolicy"
  ],
  resource
)
when {
  resource.inherited_privileges.containsAny(
    ["READ_METADATA", "BROWSE", "MANAGE", "OWNER"]
  )
};

// Changing them takes MANAGE or owning it or a container.
@id("uc-policies-change")
permit (
  principal,
  action in [
    UnityCatalog::Action::"CreatePolicy",
    UnityCatalog::Action::"UpdatePolicy",
    UnityCatalog::Action::"DeletePolicy"
  ],
  resource
)
when { resource.inherited_privileges.containsAny(["MANAGE", "OWNER"]) };

// A share's recipients: MANAGE, SET_SHARE_PERMISSION or owning the share.
@id("uc-share-permissions")
permit (
  principal,
  action in [
    UnityCatalog::Action::"GetPermissions",
    UnityCatalog::Action::"UpdatePermissions"
  ],
  resource
)
when {
  resource.inherited_privileges.containsAny(
    ["SET_SHARE_PERMISSION", "MANAGE", "OWNER"]
  )
};
