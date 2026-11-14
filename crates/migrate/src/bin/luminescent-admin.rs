//! `luminescent-admin`: manage the `authz` permission data of a Neon branch:
//! service accounts, their API keys and GitHub Actions trust, groups, and
//! grants.
//!
//! Connects to `DATABASE_URL` (`just admin <branch> …` looks up the branch's
//! direct connection string). Admins themselves are Neon Auth users with the
//! `admin` role; set that in the Neon Console or with
//! `neon neon-auth user set-role`, not here.

use anyhow::Context;
use chrono::{Duration, Utc};
use clap::{Parser, Subcommand};
use luminescent_migrate::admin::{self, GithubTrust, PrincipalRef};
use uuid::Uuid;

#[derive(Parser)]
#[command(
    version,
    about = "Manage the authz permission data of a Neon branch (DATABASE_URL)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Machine callers.
    #[command(subcommand, name = "service-account")]
    ServiceAccount(ServiceAccountCmd),
    /// API keys of service accounts.
    #[command(subcommand, name = "api-key")]
    ApiKey(ApiKeyCmd),
    /// Which GitHub Actions OIDC tokens act as a service account.
    #[command(subcommand, name = "github-trust")]
    GithubTrust(GithubTrustCmd),
    /// Groups of users, service accounts and groups.
    #[command(subcommand)]
    Group(GroupCmd),
    /// Permission tuples: subject, relation, Cedar entity.
    #[command(subcommand)]
    Grant(GrantCmd),
}

#[derive(Subcommand)]
enum ServiceAccountCmd {
    Create {
        name: String,
        #[arg(long)]
        description: Option<String>,
    },
    List,
    /// Stop the account authenticating with any credential.
    Disable {
        name: String,
    },
}

#[derive(Subcommand)]
enum ApiKeyCmd {
    /// Issue a key and print it. It is shown only this once.
    Issue {
        account: String,
        #[arg(long = "scope")]
        scopes: Vec<String>,
        /// Days until the key expires; never by default.
        #[arg(long)]
        expires_in_days: Option<u32>,
        #[arg(long)]
        description: Option<String>,
    },
    List {
        /// Only this account's keys.
        account: Option<String>,
    },
    Revoke {
        id: String,
    },
}

#[derive(Subcommand)]
enum GithubTrustCmd {
    /// Let tokens matching every given claim act as the account. Ids are
    /// GitHub's numeric ones (`gh api repos/<owner>/<repo> --jq '.owner.id, .id'`).
    Add {
        account: String,
        #[arg(long)]
        owner_id: i64,
        #[arg(long)]
        repository_id: Option<i64>,
        /// A glob over the `ref` claim, e.g. `refs/heads/main`.
        #[arg(long = "ref")]
        ref_pattern: Option<String>,
        #[arg(long)]
        environment: Option<String>,
    },
    List,
    Remove {
        id: Uuid,
    },
}

#[derive(Subcommand)]
enum GroupCmd {
    Create {
        name: String,
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete the group and every membership and grant naming it.
    Delete {
        name: String,
    },
    List,
    /// Add a member: user:<id>, service-account:<name> or group:<name>.
    Add {
        group: String,
        member: PrincipalRef,
    },
    Remove {
        group: String,
        member: PrincipalRef,
    },
    Members {
        group: String,
    },
}

#[derive(Subcommand)]
enum GrantCmd {
    /// Give a subject a relation on a Cedar entity, e.g.
    /// `grant add group:data select UnityCatalog::Catalog main`. Unity Catalog
    /// objects are named by full name (`main.sales.orders`), take one of its
    /// relations (browse, read, use, select, modify, create, manage,
    /// all_privileges, external_use) and are inherited by what they contain;
    /// grants don't follow a rename.
    Add {
        subject: PrincipalRef,
        relation: String,
        object_type: String,
        object_id: String,
    },
    Remove {
        subject: PrincipalRef,
        relation: String,
        object_type: String,
        object_id: String,
    },
    List {
        /// Only this subject's direct grants.
        subject: Option<PrincipalRef>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let url = std::env::var("DATABASE_URL")
        .context("set DATABASE_URL to the branch's connection string")?;
    let pool = luminescent_migrate::connect(&url).await?;

    match cli.command {
        Command::ServiceAccount(cmd) => match cmd {
            ServiceAccountCmd::Create { name, description } => {
                admin::create_service_account(&pool, &name, description.as_deref()).await?;
                println!("created service-account:{name}");
            }
            ServiceAccountCmd::List => {
                for a in admin::list_service_accounts(&pool).await? {
                    let state = if a.disabled { "disabled" } else { "enabled" };
                    println!(
                        "{}\t{state}\t{} live key(s)\t{}",
                        a.name,
                        a.live_keys,
                        a.description.unwrap_or_default()
                    );
                }
            }
            ServiceAccountCmd::Disable { name } => {
                admin::disable_service_account(&pool, &name).await?;
                println!("disabled service-account:{name}");
            }
        },
        Command::ApiKey(cmd) => match cmd {
            ApiKeyCmd::Issue {
                account,
                scopes,
                expires_in_days,
                description,
            } => {
                let expires_at = expires_in_days.map(|d| Utc::now() + Duration::days(d.into()));
                let key = admin::issue_api_key(
                    &pool,
                    &account,
                    &scopes,
                    expires_at,
                    description.as_deref(),
                )
                .await?;
                eprintln!(
                    "issued API key {} for service-account:{account}; store it now, it is not shown again:",
                    key.id
                );
                println!("{}", key.key);
            }
            ApiKeyCmd::List { account } => {
                for k in admin::list_api_keys(&pool, account.as_deref()).await? {
                    let state = match (k.revoked_at, k.expires_at) {
                        (Some(at), _) => format!("revoked {at}"),
                        (None, Some(at)) if at <= Utc::now() => format!("expired {at}"),
                        (None, Some(at)) => format!("expires {at}"),
                        (None, None) => "no expiry".into(),
                    };
                    println!(
                        "{}\t{}\t[{}]\tcreated {}\t{state}\t{}",
                        k.id,
                        k.account,
                        k.scopes.join(","),
                        k.created_at,
                        k.description.unwrap_or_default()
                    );
                }
            }
            ApiKeyCmd::Revoke { id } => {
                admin::revoke_api_key(&pool, &id).await?;
                println!("revoked API key {id}");
            }
        },
        Command::GithubTrust(cmd) => match cmd {
            GithubTrustCmd::Add {
                account,
                owner_id,
                repository_id,
                ref_pattern,
                environment,
            } => {
                let rule = GithubTrust {
                    repository_owner_id: owner_id,
                    repository_id,
                    ref_pattern,
                    environment,
                };
                let id = admin::add_github_trust(&pool, &account, &rule).await?;
                println!("{id}");
            }
            GithubTrustCmd::List => {
                let any = |v: Option<String>| v.unwrap_or_else(|| "*".into());
                for (id, account, r) in admin::list_github_trust(&pool).await? {
                    println!(
                        "{id}\t{account}\towner={}\trepository={}\tref={}\tenvironment={}",
                        r.repository_owner_id,
                        any(r.repository_id.map(|i| i.to_string())),
                        any(r.ref_pattern),
                        any(r.environment)
                    );
                }
            }
            GithubTrustCmd::Remove { id } => {
                admin::remove_github_trust(&pool, id).await?;
                println!("removed GitHub trust rule {id}");
            }
        },
        Command::Group(cmd) => match cmd {
            GroupCmd::Create { name, description } => {
                admin::create_group(&pool, &name, description.as_deref()).await?;
                println!("created group:{name}");
            }
            GroupCmd::Delete { name } => {
                admin::delete_group(&pool, &name).await?;
                println!("deleted group:{name}");
            }
            GroupCmd::List => {
                for (name, description, members) in admin::list_groups(&pool).await? {
                    println!(
                        "{name}\t{members} member(s)\t{}",
                        description.unwrap_or_default()
                    );
                }
            }
            GroupCmd::Add { group, member } => {
                admin::add_member(&pool, &group, &member).await?;
                println!("added {member} to group:{group}");
            }
            GroupCmd::Remove { group, member } => {
                admin::remove_member(&pool, &group, &member).await?;
                println!("removed {member} from group:{group}");
            }
            GroupCmd::Members { group } => {
                for m in admin::list_members(&pool, &group).await? {
                    println!("{m}");
                }
            }
        },
        Command::Grant(cmd) => match cmd {
            GrantCmd::Add {
                subject,
                relation,
                object_type,
                object_id,
            } => {
                admin::add_grant(&pool, &subject, &relation, &object_type, &object_id).await?;
                println!("granted {relation} on {object_type}::\"{object_id}\" to {subject}");
            }
            GrantCmd::Remove {
                subject,
                relation,
                object_type,
                object_id,
            } => {
                admin::remove_grant(&pool, &subject, &relation, &object_type, &object_id).await?;
                println!("removed {relation} on {object_type}::\"{object_id}\" from {subject}");
            }
            GrantCmd::List { subject } => {
                for g in admin::list_grants(&pool, subject.as_ref()).await? {
                    println!(
                        "{}\t{}\t{}::\"{}\"",
                        g.subject, g.relation, g.object_type, g.object_id
                    );
                }
            }
        },
    }
    pool.close().await;
    Ok(())
}
