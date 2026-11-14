// Policies that hold in every namespace.

// Neon Auth admins ("Make admin" in the Neon Console) may do anything, except
// EXTERNAL USE: handing out storage credentials always takes an explicit
// `external_use` grant, as in Databricks, where neither ownership nor ALL
// PRIVILEGES includes it.
@id("admin")
permit (principal is Lakehouse::User, action, resource)
when { principal.neon_role == "admin" }
unless { action == UnityCatalog::Action::"external_use" };

// A banned user may do nothing, admin or not.
@id("banned")
forbid (principal is Lakehouse::User, action, resource)
when { principal.banned };

// Neither may a disabled service account (authentication refuses it already).
@id("disabled")
forbid (principal is Lakehouse::ServiceAccount, action, resource)
when { principal.disabled };
