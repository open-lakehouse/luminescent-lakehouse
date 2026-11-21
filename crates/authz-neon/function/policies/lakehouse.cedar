// The lakehouse's policies over olai-pdp's built-in `Lakehouse` principals.
// olai-pdp's own baseline (built in, not in this directory) already forbids a
// banned user and a disabled service account in every namespace.

// Neon Auth admins ("Make admin" in the Neon Console) may do anything, except
// receive storage credentials: those always take an explicit EXTERNAL USE grant,
// as in Databricks, where neither ownership nor ALL PRIVILEGES includes it
// (unitycatalog.cedar's `uc-vend-admin` allows an admin's vend with the grant).
@id("admin")
permit (principal is Lakehouse::User, action, resource)
when { principal.neon_role == "admin" }
unless {
  action in
    [
      UnityCatalog::Action::"GenerateTemporaryTableCredentials",
      UnityCatalog::Action::"GenerateTemporaryVolumeCredentials",
      UnityCatalog::Action::"GenerateTemporaryModelVersionCredentials",
      UnityCatalog::Action::"GenerateTemporaryPathCredentials",
      UnityCatalog::Action::"GenerateTemporaryStagingTableCredentials",
      UnityCatalog::Action::"CreateStagingTable"
    ]
};
