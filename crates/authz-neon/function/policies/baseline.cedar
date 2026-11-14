// Policies that hold in every namespace.

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

// A banned user may do nothing, admin or not.
@id("banned")
forbid (principal is Lakehouse::User, action, resource)
when { principal.banned };

// Neither may a disabled service account (authentication refuses it already).
@id("disabled")
forbid (principal is Lakehouse::ServiceAccount, action, resource)
when { principal.disabled };
