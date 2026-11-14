// Unity Catalog decisions against the principals and grants of seed.sql: who
// may do what, and where a grant has to sit. fixtures.test.mjs runs them, with
// a sweep of every action for an admin and a user without a grant.

import { serviceAccount, uc, ucTable, user } from "../../support.mjs";

export const caller = "UNITYCATALOG";

const ADMIN = user(1);
const PLAIN = user(2);
const ANALYST = user(3); // through fixture-analysts ⊂ fixture-data
const VENDOR = user(4);
const BANNED = user(5);
const PARDONED = user(6);
const BROWSER = user(8);
const ARCHITECT = user(9);
const CREATOR = user("a");
const MANAGER = user("b");
const OWNER = user("c");
const FOUNDER = user("d");
const WRITER = user("e");
const MISGRANTED = user("f");
const ALL_PRIVILEGES = user(10);
const ADMIN_VENDOR = user(11);
const INGEST = serviceAccount(1);
const DISABLED = serviceAccount(2);

const READ = { credential_operation: "READ" };
const READ_WRITE = { credential_operation: "READ_WRITE" };

const allow = (name, request, policies) => ({ name, request, decision: "allow", policies });
const deny = (name, request) => ({ name, request, decision: "deny" });

export const cases = [
  // Metadata: usage of the catalog and schema, and a privilege on the table.
  allow("a group's grant on the schema covers its tables", ucTable(ANALYST, "GetTable"), ["uc-metadata"]),
  allow("a grant on the table itself", ucTable(VENDOR, "GetTable"), ["uc-metadata"]),
  allow("an expired ban counts for nothing", ucTable(PARDONED, "GetTable"), ["uc-metadata"]),
  allow("a service account's grants", ucTable(INGEST, "GetTable"), ["uc-metadata"]),
  deny("a disabled service account may not", ucTable(DISABLED, "GetTable")),
  deny("a user without a grant may not", ucTable(PLAIN, "GetTable")),
  deny("a banned admin may not", ucTable(BANNED, "GetTable")),
  deny("USE_SCHEMA on the table is not USE_SCHEMA on its schema", ucTable(MISGRANTED, "GetTable")),
  deny("the group's grant is on main.sales only", ucTable(ANALYST, "GetTable", "main.other.orders")),
  allow("owning the catalog covers what is in it", ucTable(OWNER, "GetTable"), ["uc-metadata"]),
  allow("ALL_PRIVILEGES on the catalog", ucTable(ALL_PRIVILEGES, "GetTable"), ["uc-metadata"]),
  allow("BROWSE on the metastore shows a table", ucTable(BROWSER, "GetTable"), ["uc-metadata"]),
  allow("BROWSE shows a schema", uc(BROWSER, "GetSchema", "Schema", "main.sales"), ["uc-metadata-schema"]),
  allow("BROWSE shows a catalog", uc(BROWSER, "GetCatalog", "Catalog", "main"), ["uc-metadata-catalog"]),
  allow("USE_CATALOG shows its catalog", uc(ANALYST, "GetCatalog", "Catalog", "main"), ["uc-metadata-catalog"]),
  deny("nothing shows another catalog", uc(ANALYST, "GetCatalog", "Catalog", "other")),

  // Lists.
  allow("anyone may list catalogs", uc(PLAIN, "ListCatalogs", "Metastore"), ["uc-list-metastore"]),
  allow("USE_CATALOG lists its schemas", uc(ANALYST, "ListSchemas", "Catalog", "main"), ["uc-list-catalog"]),
  deny("a catalog's schemas take usage", uc(PLAIN, "ListSchemas", "Catalog", "main")),
  allow("usage lists a schema's tables", uc(ANALYST, "ListTables", "Schema", "main.sales"), ["uc-list-schema"]),
  deny("a schema's tables take USE_SCHEMA on it", uc(MISGRANTED, "ListTables", "Schema", "main.sales")),
  allow("BROWSE lists a schema's tables", uc(BROWSER, "ListTables", "Schema", "main.sales"), ["uc-list-schema"]),

  // Creates.
  allow("CREATE_TABLE on the schema", ucTable(CREATOR, "CreateTable", "main.sales.new"), ["uc-create-in-schema"]),
  deny("CREATE_TABLE is not CREATE_VOLUME", uc(CREATOR, "CreateVolume", "Volume", "main.sales.files")),
  deny("SELECT is not CREATE_TABLE", ucTable(ANALYST, "CreateTable", "main.sales.new")),
  deny("CREATE_TABLE is on main.sales only", ucTable(CREATOR, "CreateTable", "main.other.new")),
  allow("owning the catalog creates in it", ucTable(OWNER, "CreateTable", "main.sales.new"), ["uc-create-in-schema"]),
  allow("CREATE_SCHEMA on the catalog", uc(ARCHITECT, "CreateSchema", "Schema", "main.new"), ["uc-create-schema"]),
  deny("CREATE_TABLE is not CREATE_SCHEMA", uc(CREATOR, "CreateSchema", "Schema", "main.new")),
  allow("ALL_PRIVILEGES creates schemas", uc(ALL_PRIVILEGES, "CreateSchema", "Schema", "main.new"), ["uc-create-schema"]),
  allow("CREATE_CATALOG on the metastore", uc(FOUNDER, "CreateCatalog", "Catalog", "new"), ["uc-create-metastore-securable"]),
  deny("owning a catalog doesn't create others", uc(OWNER, "CreateCatalog", "Catalog", "new")),
  deny("creating a catalog takes CREATE_CATALOG", uc(PLAIN, "CreateCatalog", "Catalog", "new")),

  // Changes and deletes.
  allow("MANAGE on the table", ucTable(MANAGER, "DeleteTable"), ["uc-manage"]),
  allow("owning the catalog manages its tables", ucTable(OWNER, "DeleteTable"), ["uc-manage"]),
  deny("SELECT doesn't manage", ucTable(ANALYST, "DeleteTable")),
  deny("ALL_PRIVILEGES is not MANAGE", ucTable(ALL_PRIVILEGES, "DeleteTable")),
  allow("owning the catalog manages it", uc(OWNER, "UpdateCatalog", "Catalog", "main"), ["uc-manage-metastore-securable"]),
  deny("ALL_PRIVILEGES doesn't change the catalog", uc(ALL_PRIVILEGES, "UpdateCatalog", "Catalog", "main")),
  allow("owning the catalog manages its schemas", uc(OWNER, "UpdateSchema", "Schema", "main.sales"), ["uc-manage-schema"]),

  // Data.
  allow("SELECT and MODIFY commit", ucTable(WRITER, "CommitTable"), ["uc-commit"]),
  allow("ALL_PRIVILEGES commits", ucTable(ALL_PRIVILEGES, "CommitTable"), ["uc-commit"]),
  deny("SELECT alone doesn't commit", ucTable(ANALYST, "CommitTable")),
  allow(
    "SELECT and EXTERNAL_USE_SCHEMA vend read credentials",
    ucTable(VENDOR, "GenerateTemporaryTableCredentials", undefined, READ),
    ["uc-vend"],
  ),
  deny("vending to write takes MODIFY", ucTable(VENDOR, "GenerateTemporaryTableCredentials", undefined, READ_WRITE)),
  deny("a vend names its operation", ucTable(VENDOR, "GenerateTemporaryTableCredentials")),
  deny("SELECT without EXTERNAL USE vends nothing", ucTable(ANALYST, "GenerateTemporaryTableCredentials", undefined, READ)),
  deny("ALL_PRIVILEGES is not EXTERNAL USE", ucTable(ALL_PRIVILEGES, "GenerateTemporaryTableCredentials", undefined, READ)),
  deny("an admin needs EXTERNAL USE too", ucTable(ADMIN, "GenerateTemporaryTableCredentials", undefined, READ)),
  allow(
    "an admin with EXTERNAL_USE_SCHEMA",
    ucTable(ADMIN_VENDOR, "GenerateTemporaryTableCredentials", undefined, READ_WRITE),
    ["uc-vend-admin"],
  ),
  deny(
    "an admin's EXTERNAL USE is on main.sales only",
    ucTable(ADMIN_VENDOR, "GenerateTemporaryTableCredentials", "main.other.orders", READ),
  ),
  allow(
    "READ_FILES and EXTERNAL_USE_LOCATION on the location",
    uc(INGEST, "GenerateTemporaryPathCredentials", "ExternalLocation", "landing", READ),
    ["uc-vend-path"],
  ),
  deny(
    "writing a path takes WRITE_FILES",
    uc(INGEST, "GenerateTemporaryPathCredentials", "ExternalLocation", "landing", READ_WRITE),
  ),
  deny("a path takes grants on its location", uc(PLAIN, "GenerateTemporaryPathCredentials", "ExternalLocation", "landing", READ)),

  // Staging tables.
  allow(
    "CREATE_TABLE and EXTERNAL_USE_SCHEMA reserve a staging table",
    uc(CREATOR, "CreateStagingTable", "StagingTable", "main.sales.staged"),
    ["uc-staging-create"],
  ),
  deny("reserving takes EXTERNAL USE", uc(ANALYST, "CreateStagingTable", "StagingTable", "main.sales.staged")),
  allow(
    "EXTERNAL_USE_SCHEMA vends a staging table's credentials",
    uc(CREATOR, "GenerateTemporaryStagingTableCredentials", "StagingTable", "main.sales.staged"),
    ["uc-staging-vend"],
  ),
  allow(
    "who reserved a staging table adopts it",
    uc(CREATOR, "AdoptStagingTable", "StagingTable", "main.sales.staged", { is_creator: true }),
    ["uc-staging-adopt"],
  ),
  deny(
    "nobody else does",
    uc(CREATOR, "AdoptStagingTable", "StagingTable", "main.sales.staged", { is_creator: false }),
  ),

  // Grants.
  allow("MANAGE reads a table's grants", ucTable(MANAGER, "GetGrants"), ["uc-grants-read"]),
  allow("a privilege on the table reads its grants", ucTable(VENDOR, "GetGrants"), ["uc-grants-read"]),
  deny("an inherited SELECT doesn't", ucTable(ANALYST, "GetGrants")),
  deny("nor a grant without usage", ucTable(MISGRANTED, "GetGrants")),
  allow("MANAGE changes a table's grants", ucTable(MANAGER, "UpdateGrants"), ["uc-grants-update"]),
  allow("owning the catalog changes them", ucTable(OWNER, "UpdateGrants"), ["uc-grants-update"]),
  allow("owning the catalog changes its own", uc(OWNER, "UpdateGrants", "Catalog", "main"), ["uc-grants-update"]),
  deny("ALL_PRIVILEGES changes no grant", ucTable(ALL_PRIVILEGES, "UpdateGrants")),
  deny("a privilege on the table changes no grant", ucTable(VENDOR, "UpdateGrants")),
  deny("the metastore's grants are an admin's", uc(PLAIN, "UpdateGrants", "Metastore")),

  // Tags and ABAC policies.
  allow("ALL_PRIVILEGES tags", ucTable(ALL_PRIVILEGES, "CreateEntityTagAssignment"), ["uc-tags-change"]),
  deny("SELECT doesn't tag", ucTable(ANALYST, "CreateEntityTagAssignment")),
  allow("a privilege on the table shows its tags", ucTable(VENDOR, "ListEntityTagAssignments"), ["uc-tags-read"]),
  allow("BROWSE shows tags", ucTable(BROWSER, "ListEntityTagAssignments"), ["uc-tags-read"]),
  allow("owning the catalog sets its policies", uc(OWNER, "CreatePolicy", "Catalog", "main"), ["uc-policies-change"]),
  allow("BROWSE reads policies", ucTable(BROWSER, "GetPolicy"), ["uc-policies-read"]),
  deny("SELECT reads no policy", ucTable(ANALYST, "GetPolicy")),

  allow("a privilege on the table reads its policies", ucTable(VENDOR, "ListPolicies"), ["uc-policies-read"]),
  deny("policies on a table take usage", ucTable(MISGRANTED, "GetPolicy")),
  deny("ALL_PRIVILEGES sets no policy", ucTable(ALL_PRIVILEGES, "CreatePolicy")),
  allow("MANAGE sets a table's policies", ucTable(MANAGER, "UpdatePolicy"), ["uc-policies-change"]),

  // Model versions, listed on their model.
  allow("SELECT on the schema lists a model's versions", uc(ANALYST, "ListModelVersions", "RegisteredModel", "main.sales.forecast"), [
    "uc-metadata",
  ]),
  deny("a model's versions take usage", uc(MISGRANTED, "ListModelVersions", "RegisteredModel", "main.sales.forecast")),

  // The storage proxy: data access, not EXTERNAL USE.
  allow("SELECT reads a table through the proxy", ucTable(ANALYST, "ReadData"), ["uc-proxy-read"]),
  allow("without EXTERNAL USE", ucTable(VENDOR, "ReadData"), ["uc-proxy-read"]),
  deny("reading through the proxy takes usage", ucTable(MISGRANTED, "ReadData")),
  deny("SELECT doesn't write through the proxy", ucTable(ANALYST, "WriteData")),
  allow("SELECT and MODIFY write through it", ucTable(WRITER, "WriteData"), ["uc-proxy-write"]),
  deny("a volume takes READ_VOLUME", uc(ANALYST, "ReadData", "Volume", "main.sales.files")),
  allow(
    "READ_FILES reads a location's files",
    uc(INGEST, "ReadData", "ExternalLocation", "landing", { url: "s3://landing/raw/a.parquet" }),
    ["uc-proxy-read"],
  ),
  deny("a path read names its url", uc(INGEST, "ReadData", "ExternalLocation", "landing")),
  deny(
    "writing files takes WRITE_FILES",
    uc(INGEST, "WriteData", "ExternalLocation", "landing", { url: "s3://landing/raw/a.parquet" }),
  ),

  // The metastore's securables.
  allow("USE_SHARE on the metastore shows shares", uc(FOUNDER, "GetShare", "Share", "sales"), ["uc-metadata-metastore-securable"]),
  allow("a grant on a location shows it", uc(INGEST, "GetExternalLocation", "ExternalLocation", "landing"), [
    "uc-metadata-metastore-securable",
  ]),
  deny("a share takes a grant", uc(PLAIN, "GetShare", "Share", "sales")),
  deny("USE_SHARE doesn't set a share's recipients", uc(FOUNDER, "UpdatePermissions", "Share", "sales")),
];
