mod database;
mod functions;
use clap::{Parser, Subcommand};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

const DEFAULT_API_URL: &str = "https://app.arvumi.com";
const DEPLOYMENT_DOMAIN: &str = "arvumi.app";

#[derive(Parser)]
#[command(
    name = "arvumi",
    version,
    about = "Deploy apps with Arvumi, your deployment platform"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Configure project defaults for serverless functions
    Functions {
        #[command(subcommand)]
        action: functions::Action,
    },
    /// List repositories granted to a team
    GithubRepos {
        #[arg(long)]
        team: String,
    },
    /// Create a project from a repository already granted in team settings
    CreateProject {
        name: String,
        #[arg(long)]
        team: String,
        #[arg(long)]
        github_repository_grant: String,
        #[arg(long, default_value = "main")]
        branch: String,
    },
    /// Access hosted PostgreSQL through an authenticated private tunnel
    Db {
        #[command(subcommand)]
        action: database::Action,
    },
    /// Configure Arvumi with your API endpoint and key
    Init {
        #[arg(long)]
        api: Option<String>,
        #[arg(long)]
        key: Option<String>,
    },
    /// Sign in with GitHub via browser
    Login {
        /// API URL (defaults to current config or https://app.arvumi.com)
        #[arg(long)]
        api: Option<String>,
    },
    /// Trigger a deployment from the project's connected Git repository
    Deploy {
        /// Project ID (optional, reads from .arvumi file if exists)
        #[arg(long)]
        project: Option<String>,
        /// Branch to deploy (defaults to current git branch)
        #[arg(long)]
        branch: Option<String>,
        /// Commit SHA (optional)
        #[arg(long)]
        commit: Option<String>,
        /// Commit message (optional)
        #[arg(long, short)]
        message: Option<String>,
    },
    /// Link the current directory to a project
    Link { project_id: String },
    /// Stream build logs for a deployment
    Logs { deployment_id: String },
    /// List deployments for a project
    List {
        /// Project ID
        project_id: String,
    },
    /// Show current configuration
    Config,
    /// Manage environment variables for a project
    Env {
        /// Variable scope: production, preview, or an environment UUID
        #[arg(long)]
        environment: String,
        /// Project ID (optional, reads from .arvumi file if exists)
        #[arg(long)]
        project: Option<String>,
        #[command(subcommand)]
        action: EnvAction,
    },
    /// Manage custom domains for a project
    Domain {
        /// Project ID (optional, reads from .arvumi file if exists)
        #[arg(long)]
        project: Option<String>,
        #[command(subcommand)]
        action: DomainAction,
    },
    /// List project environments and manage custom environments
    Environments {
        /// Project ID (optional, reads from .arvumi file if exists)
        #[arg(long)]
        project: Option<String>,
        #[command(subcommand)]
        action: EnvironmentsAction,
    },
    /// Manage teams
    Teams {
        #[command(subcommand)]
        action: TeamAction,
    },
}

#[derive(Subcommand)]
enum DomainAction {
    /// List custom domains
    List,
    /// Add a custom domain
    Add {
        domain: String,
        #[arg(long, default_value="cname", value_parser=["cname","flattened"])]
        mode: String,
        /// Configured environment UUID (defaults to Production)
        #[arg(long)]
        environment: Option<uuid::Uuid>,
    },
    /// Change the environment served by an existing domain
    Target {
        domain: String,
        #[arg(
            long,
            required_unless_present = "production",
            conflicts_with = "production"
        )]
        environment: Option<uuid::Uuid>,
        #[arg(long)]
        production: bool,
    },
    /// Remove a custom domain
    Remove { domain: String },
    /// Recheck DNS and optionally wait up to five minutes for activation
    Check {
        domain: String,
        #[arg(long)]
        wait: bool,
    },
}

#[derive(Subcommand)]
enum EnvAction {
    /// List environment variables
    List,
    /// Set an environment variable
    Set { key: String, value: String },
    /// Delete an environment variable
    Delete { key: String },
}

#[derive(Subcommand)]
enum EnvironmentsAction {
    /// List Production, Preview, and custom environments
    List,
    /// Create an environment
    Create {
        /// Display name (e.g. "staging")
        #[arg(long)]
        name: String,
        /// Git branch to track
        #[arg(long)]
        branch: String,
        /// Subdomain prefix (e.g. "staging-myapp")
        #[arg(long)]
        subdomain: String,
    },
    /// Delete an environment
    Delete {
        /// Environment ID
        id: String,
    },
}

#[derive(Subcommand)]
enum TeamAction {
    /// List your teams
    List,
    /// Create a new team
    Create {
        /// Team name
        name: String,
    },
    /// Switch active team
    Switch {
        /// Team slug
        slug: String,
    },
    /// List team members
    Members,
    /// Invite a user to your team
    Invite {
        /// Email address
        email: String,
        /// Role (admin, member, viewer)
        #[arg(long, default_value = "member")]
        role: String,
    },
    /// Remove a member from your team
    Remove {
        /// GitHub login of the member
        user: String,
    },
}

#[derive(Serialize, Deserialize)]
struct ArvumiConfig {
    api_url: String,
    api_key: Option<String>,
    access_token: Option<String>,
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("arvumi")
        .join("config.json")
}

fn load_config() -> eyre::Result<ArvumiConfig> {
    read_config(&config_path())
}

fn read_config(path: &Path) -> eyre::Result<ArvumiConfig> {
    use eyre::WrapErr;
    let content = std::fs::read_to_string(path).wrap_err(
        "Could not read config. Run 'arvumi init' or 'arvumi login --api <url>' first.",
    )?;
    Ok(serde_json::from_str(&content)?)
}

fn get_auth_header(config: &ArvumiConfig) -> Option<String> {
    if let Some(ref tok) = config.access_token
        && !tok.is_empty()
    {
        return Some(format!("Bearer {}", tok));
    }
    if let Some(ref key) = config.api_key
        && !key.is_empty()
    {
        return Some(format!("Bearer {}", key));
    }
    None
}

fn require_auth_header(config: &ArvumiConfig) -> String {
    get_auth_header(config).unwrap_or_else(|| {
        eprintln!("No auth configured. Run 'arvumi login' or 'arvumi init --key <key>'.");
        std::process::exit(1);
    })
}

fn save_config(config: &ArvumiConfig) -> eyre::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options.open(&path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(serde_json::to_string_pretty(config)?.as_bytes())?;
    Ok(())
}

fn project_link_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".arvumi")
}

fn read_project_link() -> Option<String> {
    let path = project_link_path();
    if path.exists() {
        std::fs::read_to_string(&path)
            .ok()
            .map(|s| s.trim().to_string())
    } else {
        None
    }
}

fn write_project_link(project_id: &str) -> eyre::Result<()> {
    std::fs::write(project_link_path(), project_id)?;
    Ok(())
}

fn current_git_branch() -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok()
            } else {
                None
            }
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "HEAD")
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Functions { action } => functions::run(action).await,
        Commands::GithubRepos { team } => cmd_github_repos(&team).await,
        Commands::CreateProject {
            name,
            team,
            github_repository_grant,
            branch,
        } => cmd_create_project(&name, &team, &github_repository_grant, &branch).await,
        Commands::Db { action } => database::run(action).await,
        Commands::Init { api, key } => cmd_init(api, key).await,
        Commands::Login { api } => cmd_login(api).await,
        Commands::Deploy {
            project,
            branch,
            commit,
            message,
        } => cmd_deploy(project, branch, commit, message).await,
        Commands::Link { project_id } => cmd_link(&project_id),
        Commands::Logs { deployment_id } => cmd_logs(&deployment_id).await,
        Commands::List { project_id } => cmd_list(&project_id).await,
        Commands::Config => cmd_config(),
        Commands::Env {
            project,
            environment,
            action,
        } => cmd_env(project, environment, action).await,
        Commands::Domain { project, action } => cmd_domain(project, action).await,
        Commands::Environments { project, action } => cmd_environments(project, action).await,
        Commands::Teams { action } => cmd_teams(action).await,
    }
}

async fn cmd_init(api: Option<String>, key: Option<String>) -> eyre::Result<()> {
    let api_url = api.unwrap_or_else(|| DEFAULT_API_URL.to_string());

    let api_key = key.unwrap_or_else(|| {
        dialoguer::Password::new()
            .with_prompt("API Key (leave blank to sign in with GitHub)")
            .allow_empty_password(true)
            .interact()
            .unwrap_or_default()
    });

    let config = ArvumiConfig {
        api_url: api_url.clone(),
        api_key: if api_key.is_empty() {
            None
        } else {
            Some(api_key)
        },
        access_token: None,
    };
    save_config(&config)?;
    println!("Configuration saved to {}", config_path().display());

    if config.api_key.is_none() {
        return cmd_login(Some(api_url)).await;
    }
    Ok(())
}

fn login_api_url(api: Option<String>, path: &Path) -> eyre::Result<String> {
    if let Some(api) = api {
        return Ok(api);
    }
    if path.try_exists()? {
        return Ok(read_config(path)?.api_url);
    }
    Ok(DEFAULT_API_URL.to_string())
}

async fn cmd_login(api: Option<String>) -> eyre::Result<()> {
    let api_url = login_api_url(api, &config_path())?;

    let login_url = format!("{}/login?src=cli", api_url.trim_end_matches('/'));

    eprintln!("Opening browser to sign in with GitHub...");
    eprintln!();

    if let Err(e) = open::that(&login_url) {
        eprintln!("Failed to open browser: {}", e);
        eprintln!("Please visit: {}", login_url);
    }

    eprintln!("After signing in, a CLI key will be displayed.");
    eprintln!("Copy the 'arvumi init --key ...' command and run it in your terminal.");
    Ok(())
}

async fn cmd_deploy(
    project: Option<String>,
    branch: Option<String>,
    commit: Option<String>,
    message: Option<String>,
) -> eyre::Result<()> {
    let config = load_config()?;
    let client = reqwest::Client::new();

    let project_id = project.or_else(read_project_link).unwrap_or_else(|| {
        eprintln!("No project specified. Use --project or run 'arvumi link <project-id>' first.");
        std::process::exit(1);
    });

    let branch = branch.or_else(current_git_branch);

    let url = format!(
        "{}/api/projects/{}/deployments",
        config.api_url.trim_end_matches('/'),
        project_id
    );

    let mut body = serde_json::json!({
        "commit_sha": commit,
        "commit_message": message.unwrap_or_else(|| "CLI deploy".to_string()),
    });
    if let Some(ref b) = branch {
        body["branch"] = serde_json::Value::String(b.clone());
    }

    let resp = client
        .post(&url)
        .header(AUTHORIZATION, require_auth_header(&config))
        .header(CONTENT_TYPE, "application/json")
        .json(&body)
        .send()
        .await?;

    if resp.status().is_success() {
        let deployment: serde_json::Value = resp.json().await?;
        let id = deployment["id"].as_str().unwrap_or("unknown").to_string();
        let domain = deployment["domain"].as_str().unwrap_or("unknown");
        println!("Deployment created: {}", id);
        println!("URL: https://{}", domain);
        println!("\nStreaming logs...\n");
        stream_logs(&config, &id).await?;
    } else {
        let error = resp.text().await?;
        // A "dedicated environment URL" skip surfaces as a 400 with a message
        if error.contains("dedicated environment URL") {
            eprintln!("Skipped: {}", error);
        } else {
            eprintln!("Error: {}", error);
        }
        std::process::exit(1);
    }

    Ok(())
}

fn cmd_link(project_id: &str) -> eyre::Result<()> {
    write_project_link(project_id)?;
    println!("Linked to project {}", project_id);
    Ok(())
}

async fn cmd_logs(deployment_id: &str) -> eyre::Result<()> {
    let config = load_config()?;
    stream_logs(&config, deployment_id).await
}

async fn stream_logs(config: &ArvumiConfig, deployment_id: &str) -> eyre::Result<()> {
    let client = reqwest::Client::new();
    let logs_url = format!(
        "{}/api/deployments/{}/logs",
        config.api_url.trim_end_matches('/'),
        deployment_id
    );
    let status_url = format!(
        "{}/api/deployments/{}/status",
        config.api_url.trim_end_matches('/'),
        deployment_id
    );
    let auth = require_auth_header(config);

    let mut seen = 0usize;
    loop {
        let resp = client
            .get(&logs_url)
            .header(AUTHORIZATION, &auth)
            .send()
            .await?;

        if resp.status().is_success() {
            let logs: Vec<serde_json::Value> = resp.json().await?;
            for log in logs.iter().skip(seen) {
                println!("{}", log["line"].as_str().unwrap_or(""));
            }
            seen = logs.len();
        }

        let status_resp = client
            .get(&status_url)
            .header(AUTHORIZATION, &auth)
            .send()
            .await?;

        if status_resp.status().is_success() {
            let status: serde_json::Value = status_resp.json().await?;
            let s = status["status"].as_str().unwrap_or("building");
            if s == "ready" {
                println!("\nBuild succeeded.");
                break;
            }
            if s == "failed" {
                println!("\nBuild failed.");
                std::process::exit(1);
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    Ok(())
}

async fn cmd_list(project_id: &str) -> eyre::Result<()> {
    let config = load_config()?;
    let client = reqwest::Client::new();

    let url = format!(
        "{}/api/projects/{}/deployments",
        config.api_url.trim_end_matches('/'),
        project_id
    );

    let resp = client
        .get(&url)
        .header(AUTHORIZATION, require_auth_header(&config))
        .send()
        .await?;

    if resp.status().is_success() {
        let deployments: Vec<serde_json::Value> = resp.json().await?;
        if deployments.is_empty() {
            println!("No deployments found.");
        } else {
            println!(
                "{:<12} {:<12} {:<36} {:<}",
                "STATUS", "ID", "DOMAIN", "DATE"
            );
            println!("{}", "-".repeat(80));
            for dep in &deployments {
                let status = dep["status"].as_str().unwrap_or("?");
                let id = dep["id"].as_str().unwrap_or("?");
                let domain = dep["domain"].as_str().unwrap_or("?");
                let _ts = dep["created_at"].as_i64().unwrap_or(0);
                println!("{:<12} {:<12} {:<36}", status, id, domain);
            }
        }
    } else {
        let error = resp.text().await?;
        eprintln!("Error: {}", error);
        std::process::exit(1);
    }

    Ok(())
}

async fn cmd_env(
    project: Option<String>,
    environment: String,
    action: EnvAction,
) -> eyre::Result<()> {
    let config = load_config()?;
    let client = reqwest::Client::new();

    let project_id = project.or_else(read_project_link).unwrap_or_else(|| {
        eprintln!("No project specified. Use --project or run 'arvumi link <project-id>' first.");
        std::process::exit(1);
    });

    match action {
        EnvAction::List => {
            let url = format!(
                "{}/api/projects/{}/env",
                config.api_url.trim_end_matches('/'),
                project_id
            );
            let resp = client
                .get(&url)
                .query(&[("environment", &environment)])
                .header(AUTHORIZATION, require_auth_header(&config))
                .send()
                .await?;

            if resp.status().is_success() {
                let vars: Vec<serde_json::Value> = resp.json().await?;
                if vars.is_empty() {
                    println!("No environment variables configured.");
                } else {
                    for v in &vars {
                        println!(
                            "{}={}",
                            v["key"].as_str().unwrap_or("?"),
                            v["value"].as_str().unwrap_or("<hidden>")
                        );
                    }
                }
            } else {
                let error = resp.text().await?;
                eprintln!("Error: {}", error);
                std::process::exit(1);
            }
        }
        EnvAction::Set { key, value } => {
            let url = format!(
                "{}/api/projects/{}/env",
                config.api_url.trim_end_matches('/'),
                project_id
            );
            let body =
                serde_json::json!({ "key": &key, "value": &value, "environment": &environment });
            let resp = client
                .post(&url)
                .header(AUTHORIZATION, require_auth_header(&config))
                .header(CONTENT_TYPE, "application/json")
                .json(&body)
                .send()
                .await?;

            if resp.status().is_success() {
                println!("Set {} in {}", key, environment);
            } else {
                let error = resp.text().await?;
                eprintln!("Error: {}", error);
                std::process::exit(1);
            }
        }
        EnvAction::Delete { key } => {
            let list_url = format!(
                "{}/api/projects/{}/env",
                config.api_url.trim_end_matches('/'),
                project_id
            );
            let resp = client
                .get(&list_url)
                .query(&[("environment", &environment)])
                .header(AUTHORIZATION, require_auth_header(&config))
                .send()
                .await?;

            if !resp.status().is_success() {
                eprintln!("Error listing env vars");
                std::process::exit(1);
            }

            let vars: Vec<serde_json::Value> = resp.json().await?;
            let target = vars.iter().find(|v| v["key"].as_str() == Some(&key));

            let var_id = match target {
                Some(v) => v["id"].as_str().unwrap_or(""),
                None => {
                    eprintln!("Env var '{}' not found", key);
                    std::process::exit(1);
                }
            };

            let delete_url = format!(
                "{}/api/projects/{}/env/{}",
                config.api_url.trim_end_matches('/'),
                project_id,
                var_id
            );
            let resp = client
                .delete(&delete_url)
                .query(&[("environment", &environment)])
                .header(AUTHORIZATION, require_auth_header(&config))
                .send()
                .await?;

            if resp.status().is_success() {
                println!("Deleted {}", key);
            } else {
                let error = resp.text().await?;
                eprintln!("Error: {}", error);
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

async fn cmd_domain(project: Option<String>, action: DomainAction) -> eyre::Result<()> {
    let config = load_config()?;
    let client = reqwest::Client::new();

    let project_id = project.or_else(read_project_link).unwrap_or_else(|| {
        eprintln!("No project specified. Use --project or run 'arvumi link <project-id>' first.");
        std::process::exit(1);
    });

    match action {
        DomainAction::List => {
            let url = format!(
                "{}/api/projects/{}/domains",
                config.api_url.trim_end_matches('/'),
                project_id
            );
            let resp = client
                .get(&url)
                .header(AUTHORIZATION, require_auth_header(&config))
                .send()
                .await?;

            if resp.status().is_success() {
                let domains: Vec<serde_json::Value> = resp.json().await?;
                if domains.is_empty() {
                    println!("No custom domains configured.");
                } else {
                    for d in &domains {
                        let status = d["status"].as_str().unwrap_or("unknown");
                        println!(
                            "{} [{}] → {}",
                            d["domain"].as_str().unwrap_or("?"),
                            status,
                            d["environment_id"].as_str().unwrap_or("Production")
                        );
                    }
                }
            } else {
                let error = resp.text().await?;
                eprintln!("Error: {}", error);
                std::process::exit(1);
            }
        }
        DomainAction::Add {
            domain,
            mode,
            environment,
        } => {
            let url = format!(
                "{}/api/projects/{}/domains",
                config.api_url.trim_end_matches('/'),
                project_id
            );
            let body = serde_json::json!({ "domain": &domain, "mode": mode, "environment_id": environment });
            let resp = client
                .post(&url)
                .header(AUTHORIZATION, require_auth_header(&config))
                .header(CONTENT_TYPE, "application/json")
                .json(&body)
                .send()
                .await?;

            if resp.status().is_success() {
                let d: serde_json::Value = resp.json().await?;
                println!("Added domain: {}", domain);
                if let Some(records) = d["records"].as_array() {
                    for r in records {
                        println!(
                            "{} {} → {}",
                            r["record_type"].as_str().unwrap_or("?"),
                            r["name"].as_str().unwrap_or("?"),
                            r["value"].as_str().unwrap_or("?")
                        );
                    }
                }
                println!("DNS and HTTPS are checked automatically.");
            } else {
                let error = resp.text().await?;
                eprintln!("Error: {}", error);
                std::process::exit(1);
            }
        }
        DomainAction::Target {
            domain,
            environment,
            production: _,
        } => {
            let url = format!(
                "{}/api/projects/{}/domains",
                config.api_url.trim_end_matches('/'),
                project_id
            );
            let response = client
                .get(&url)
                .header(AUTHORIZATION, require_auth_header(&config))
                .send()
                .await?;
            if !response.status().is_success() {
                eyre::bail!("Error: {}", response.text().await?);
            }
            let domains: Vec<serde_json::Value> = response.json().await?;
            let id = domains
                .iter()
                .find(|d| d["domain"].as_str() == Some(&domain))
                .and_then(|d| d["id"].as_str())
                .ok_or_else(|| eyre::eyre!("Domain '{}' not found", domain))?;
            let response = client
                .patch(format!("{url}/{id}"))
                .header(AUTHORIZATION, require_auth_header(&config))
                .json(&serde_json::json!({"environment_id": environment}))
                .send()
                .await?;
            if !response.status().is_success() {
                eyre::bail!("Error: {}", response.text().await?);
            }
            println!(
                "{} now follows {}",
                domain,
                environment
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "Production".into())
            );
        }
        DomainAction::Remove { domain } => {
            let list_url = format!(
                "{}/api/projects/{}/domains",
                config.api_url.trim_end_matches('/'),
                project_id
            );
            let resp = client
                .get(&list_url)
                .header(AUTHORIZATION, require_auth_header(&config))
                .send()
                .await?;

            if !resp.status().is_success() {
                eprintln!("Error listing domains");
                std::process::exit(1);
            }

            let domains: Vec<serde_json::Value> = resp.json().await?;
            let target = domains
                .iter()
                .find(|d| d["domain"].as_str() == Some(&domain));

            let domain_id = match target {
                Some(d) => d["id"].as_str().unwrap_or(""),
                None => {
                    eprintln!("Domain '{}' not found", domain);
                    std::process::exit(1);
                }
            };

            let delete_url = format!(
                "{}/api/projects/{}/domains/{}",
                config.api_url.trim_end_matches('/'),
                project_id,
                domain_id
            );
            let resp = client
                .delete(&delete_url)
                .header(AUTHORIZATION, require_auth_header(&config))
                .send()
                .await?;

            if resp.status().is_success() {
                println!("Removed domain: {}", domain);
            } else {
                let error = resp.text().await?;
                eprintln!("Error: {}", error);
                std::process::exit(1);
            }
        }
        DomainAction::Check { domain, wait } => {
            let list_url = format!(
                "{}/api/projects/{}/domains",
                config.api_url.trim_end_matches('/'),
                project_id
            );
            let resp = client
                .get(&list_url)
                .header(AUTHORIZATION, require_auth_header(&config))
                .send()
                .await?;

            if !resp.status().is_success() {
                eprintln!("Error listing domains");
                std::process::exit(1);
            }

            let domains: Vec<serde_json::Value> = resp.json().await?;
            let target = domains
                .iter()
                .find(|d| d["domain"].as_str() == Some(&domain));

            let domain_id = match target {
                Some(d) => d["id"].as_str().unwrap_or(""),
                None => {
                    eprintln!("Domain '{}' not found", domain);
                    std::process::exit(1);
                }
            };

            let verify_url = format!(
                "{}/api/projects/{}/domains/{}/check",
                config.api_url.trim_end_matches('/'),
                project_id,
                domain_id
            );
            let resp = client
                .post(&verify_url)
                .header(AUTHORIZATION, require_auth_header(&config))
                .send()
                .await?;

            if resp.status().is_success() {
                println!("DNS check queued for {}", domain);
                if wait {
                    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
                    loop {
                        let values: Vec<serde_json::Value> = client
                            .get(&list_url)
                            .header(AUTHORIZATION, require_auth_header(&config))
                            .send()
                            .await?
                            .error_for_status()?
                            .json()
                            .await?;
                        let value = values
                            .iter()
                            .find(|v| v["id"].as_str() == Some(domain_id))
                            .ok_or_else(|| eyre::eyre!("domain was removed"))?;
                        let status = value["status"].as_str().unwrap_or("unknown");
                        println!("{}: {}", domain, status);
                        if status == "active" {
                            break;
                        }
                        if std::time::Instant::now() >= deadline {
                            return Err(eyre::eyre!(
                                "domain is still {}; automatic checks continue",
                                status
                            ));
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            } else {
                let error = resp.text().await?;
                eprintln!("Error: {}", error);
                std::process::exit(1);
            }
        }
    }

    Ok(())
}

async fn cmd_environments(project: Option<String>, action: EnvironmentsAction) -> eyre::Result<()> {
    let config = load_config()?;
    let client = reqwest::Client::new();

    let project_id = project.or_else(read_project_link).unwrap_or_else(|| {
        eprintln!("No project specified. Use --project or run 'arvumi link <project-id>' first.");
        std::process::exit(1);
    });

    match action {
        EnvironmentsAction::List => {
            let url = format!(
                "{}/api/projects/{}/environments",
                config.api_url.trim_end_matches('/'),
                project_id
            );
            let resp = client
                .get(&url)
                .header(AUTHORIZATION, require_auth_header(&config))
                .send()
                .await?;

            if resp.status().is_success() {
                let environments: Vec<serde_json::Value> = resp.json().await?;
                #[derive(Deserialize)]
                struct ProjectEnvironmentInfo {
                    name: String,
                    production_branch: String,
                }
                let project: ProjectEnvironmentInfo = client
                    .get(format!(
                        "{}/api/projects/{}",
                        config.api_url.trim_end_matches('/'),
                        project_id
                    ))
                    .header(AUTHORIZATION, require_auth_header(&config))
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;
                let production_url = format!(
                    "https://{}.{}",
                    project.name.to_ascii_lowercase(),
                    DEPLOYMENT_DOMAIN
                );
                println!("{:<30} {:<20} {:<30}", "NAME", "BRANCH", "URL");
                println!(
                    "{:<30} {:<20} {:<30}",
                    "Production", project.production_branch, production_url
                );
                println!(
                    "{:<30} {:<20} {:<30}",
                    "Preview", "Other branches", "Per deployment"
                );
                for e in &environments {
                    let name = e["name"].as_str().unwrap_or("?");
                    let branch = e["branch"].as_str().unwrap_or("?");
                    let subdomain = e["subdomain"].as_str().unwrap_or("?");
                    let url = format!("{}.{}", subdomain, DEPLOYMENT_DOMAIN);
                    println!("{:<30} {:<20} {:<30}", name, branch, url);
                }
                println!(
                    "\nProduction and Preview are built-in environments (scopes: production, preview)."
                );
                println!(
                    "Branches use Preview unless they are the production branch or have a custom environment."
                );
            } else {
                let error = resp.text().await?;
                eprintln!("Error: {}", error);
                std::process::exit(1);
            }
        }
        EnvironmentsAction::Create {
            name,
            branch,
            subdomain,
        } => {
            let url = format!(
                "{}/api/projects/{}/environments",
                config.api_url.trim_end_matches('/'),
                project_id
            );
            let body = serde_json::json!({
                "name": &name,
                "branch": &branch,
                "subdomain": &subdomain,
            });
            let resp = client
                .post(&url)
                .header(AUTHORIZATION, require_auth_header(&config))
                .header(CONTENT_TYPE, "application/json")
                .json(&body)
                .send()
                .await?;

            if resp.status().is_success() {
                let e: serde_json::Value = resp.json().await?;
                let sub = e["subdomain"].as_str().unwrap_or("?");
                println!(
                    "Created environment '{}' for branch '{}' at {}.{}",
                    name, branch, sub, DEPLOYMENT_DOMAIN
                );
            } else {
                let error = resp.text().await?;
                eprintln!("Error: {}", error);
                std::process::exit(1);
            }
        }
        EnvironmentsAction::Delete { id } => {
            let url = format!(
                "{}/api/projects/{}/environments/{}",
                config.api_url.trim_end_matches('/'),
                project_id,
                id
            );
            let resp = client
                .delete(&url)
                .header(AUTHORIZATION, require_auth_header(&config))
                .send()
                .await?;

            if resp.status().is_success() {
                println!("Removed environment {}", id);
            } else {
                let error = resp.text().await?;
                eprintln!("Error: {}", error);
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn cmd_config() -> eyre::Result<()> {
    let config = load_config()?;
    println!("API URL: {}", config.api_url);
    println!(
        "API Key: {}",
        if config.api_key.as_ref().is_some_and(|key| !key.is_empty()) {
            "configured"
        } else {
            "not configured"
        }
    );
    if config
        .access_token
        .as_ref()
        .is_some_and(|token| !token.is_empty())
    {
        println!("Session: configured");
    }
    Ok(())
}

async fn cmd_teams(action: TeamAction) -> eyre::Result<()> {
    let config = load_config()?;
    let client = reqwest::Client::new();
    let auth = require_auth_header(&config);
    let base = config.api_url.trim_end_matches('/');

    match action {
        TeamAction::List => {
            let resp = client
                .get(format!("{}/api/teams", base))
                .header(AUTHORIZATION, &auth)
                .send()
                .await?;
            if resp.status().is_success() {
                let teams: Vec<serde_json::Value> = resp.json().await?;
                if teams.is_empty() {
                    println!("No teams found.");
                } else {
                    for t in &teams {
                        println!(
                            "{} [{}]",
                            t["name"].as_str().unwrap_or("?"),
                            t["slug"].as_str().unwrap_or("?")
                        );
                    }
                }
            } else {
                let err = resp.text().await?;
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
        }
        TeamAction::Create { name } => {
            let body = serde_json::json!({"name": &name});
            let resp = client
                .post(format!("{}/api/teams", base))
                .header(AUTHORIZATION, &auth)
                .header(CONTENT_TYPE, "application/json")
                .json(&body)
                .send()
                .await?;
            if resp.status().is_success() {
                let team: serde_json::Value = resp.json().await?;
                println!(
                    "Created team '{}' [{}]",
                    team["name"].as_str().unwrap_or("?"),
                    team["slug"].as_str().unwrap_or("?")
                );
            } else {
                let err = resp.text().await?;
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
        }
        TeamAction::Switch { slug } => {
            let body = serde_json::json!({"slug": &slug});
            let resp = client
                .post(format!("{}/api/teams/switch", base))
                .header(AUTHORIZATION, &auth)
                .header(CONTENT_TYPE, "application/json")
                .json(&body)
                .send()
                .await?;
            if resp.status().is_success() {
                println!("Switched to team '{}'", slug);
            } else {
                let err = resp.text().await?;
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
        }
        TeamAction::Members => {
            let teams_resp = client
                .get(format!("{}/api/teams", base))
                .header(AUTHORIZATION, &auth)
                .send()
                .await?;
            if !teams_resp.status().is_success() {
                eprintln!("Error fetching teams");
                std::process::exit(1);
            }
            let teams: Vec<serde_json::Value> = teams_resp.json().await?;
            let team_slug = teams.first().and_then(|t| t["slug"].as_str()).unwrap_or("");
            if team_slug.is_empty() {
                eprintln!("No teams found");
                std::process::exit(1);
            }
            let resp = client
                .get(format!("{}/api/teams/{}/members", base, team_slug))
                .header(AUTHORIZATION, &auth)
                .send()
                .await?;
            if resp.status().is_success() {
                let members: Vec<serde_json::Value> = resp.json().await?;
                for m in &members {
                    println!(
                        "{} [{}]",
                        m["user_login"].as_str().unwrap_or("?"),
                        m["role"].as_str().unwrap_or("?")
                    );
                }
            } else {
                let err = resp.text().await?;
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
        }
        TeamAction::Invite { email, role } => {
            let teams_resp = client
                .get(format!("{}/api/teams", base))
                .header(AUTHORIZATION, &auth)
                .send()
                .await?;
            let teams: Vec<serde_json::Value> = teams_resp.json().await?;
            let team_slug = teams.first().and_then(|t| t["slug"].as_str()).unwrap_or("");
            if team_slug.is_empty() {
                eprintln!("No teams found");
                std::process::exit(1);
            }
            let body = serde_json::json!({"email": &email, "role": &role});
            let resp = client
                .post(format!("{}/api/teams/{}/invitations", base, team_slug))
                .header(AUTHORIZATION, &auth)
                .header(CONTENT_TYPE, "application/json")
                .json(&body)
                .send()
                .await?;
            if resp.status().is_success() {
                let inv: serde_json::Value = resp.json().await?;
                let token = inv["token"].as_str().unwrap_or("?");
                println!(
                    "Invited {} as {}. Accept URL: {}/api/invitations/{}/accept",
                    email, role, base, token
                );
            } else {
                let err = resp.text().await?;
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
        }
        TeamAction::Remove { user } => {
            let teams_resp = client
                .get(format!("{}/api/teams", base))
                .header(AUTHORIZATION, &auth)
                .send()
                .await?;
            let teams: Vec<serde_json::Value> = teams_resp.json().await?;
            let team_slug = teams.first().and_then(|t| t["slug"].as_str()).unwrap_or("");
            if team_slug.is_empty() {
                eprintln!("No teams found");
                std::process::exit(1);
            }
            let member_resp = client
                .get(format!("{}/api/teams/{}/members", base, team_slug))
                .header(AUTHORIZATION, &auth)
                .send()
                .await?;
            let members: Vec<serde_json::Value> = member_resp.json().await?;
            let target = members
                .iter()
                .find(|m| m["user_login"].as_str() == Some(&user));
            let member_id = match target.and_then(|m| m["user_id"].as_str()) {
                Some(id) => id,
                None => {
                    eprintln!("Member '{}' not found", user);
                    std::process::exit(1);
                }
            };
            let resp = client
                .delete(format!(
                    "{}/api/teams/{}/members/{}",
                    base, team_slug, member_id
                ))
                .header(AUTHORIZATION, &auth)
                .send()
                .await?;
            if resp.status().is_success() {
                println!("Removed {} from team", user);
            } else {
                let err = resp.text().await?;
                eprintln!("Error: {}", err);
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

async fn cmd_github_repos(team: &str) -> eyre::Result<()> {
    let config = load_config()?;
    let response = reqwest::Client::new()
        .get(format!(
            "{}/api/teams/{team}/github/grants",
            config.api_url.trim_end_matches('/')
        ))
        .header(AUTHORIZATION, require_auth_header(&config))
        .send()
        .await?;
    if !response.status().is_success() {
        eyre::bail!("repository listing failed: {}", response.status());
    }
    let rows: serde_json::Value = response.json().await?;
    for row in rows
        .as_array()
        .ok_or_else(|| eyre::eyre!("invalid repository response"))?
    {
        println!(
            "{}  {}  {}/{}",
            row["id"].as_str().unwrap_or(""),
            row["full_name"].as_str().unwrap_or(""),
            row["state"].as_str().unwrap_or(""),
            row["connection_state"].as_str().unwrap_or("")
        );
    }
    Ok(())
}
async fn cmd_create_project(name: &str, team: &str, grant: &str, branch: &str) -> eyre::Result<()> {
    let config = load_config()?;
    let response=reqwest::Client::new().post(format!("{}/api/projects",config.api_url.trim_end_matches('/'))).header(AUTHORIZATION,require_auth_header(&config)).json(&serde_json::json!({"name":name,"team_id":team,"github_repository_grant_id":grant,"branch":branch})).send().await?;
    if !response.status().is_success() {
        eyre::bail!("project creation failed: {}", response.status());
    }
    let project: serde_json::Value = response.json().await?;
    println!("Created project {}", project["id"].as_str().unwrap_or(""));
    Ok(())
}

#[cfg(test)]
mod env_scope_tests {
    use super::*;

    #[test]
    fn domain_target_requires_explicit_environment_or_production() {
        let env = uuid::Uuid::new_v4().to_string();
        assert!(Cli::try_parse_from(["arvumi", "domain", "target", "www.example.com"]).is_err());
        assert!(
            Cli::try_parse_from([
                "arvumi",
                "domain",
                "target",
                "www.example.com",
                "--environment",
                &env,
                "--production"
            ])
            .is_err()
        );
        assert!(
            Cli::try_parse_from([
                "arvumi",
                "domain",
                "target",
                "www.example.com",
                "--production"
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "arvumi",
                "domain",
                "target",
                "www.example.com",
                "--environment",
                &env
            ])
            .is_ok()
        );
        assert!(
            Cli::try_parse_from([
                "arvumi",
                "domain",
                "add",
                "www.example.com",
                "--environment",
                &env
            ])
            .is_ok()
        );
    }

    #[test]
    fn env_commands_require_scope() {
        for action in [
            vec!["list"],
            vec!["set", "KEY", "value"],
            vec!["delete", "KEY"],
        ] {
            let mut missing = vec!["arvumi", "env"];
            missing.extend(action.clone());
            assert!(Cli::try_parse_from(missing).is_err());
            let mut scoped = vec!["arvumi", "env", "--environment", "preview"];
            scoped.extend(action);
            match Cli::try_parse_from(scoped).unwrap().command {
                Commands::Env { environment, .. } => assert_eq!(environment, "preview"),
                _ => panic!("expected env command"),
            }
        }
    }
}

#[cfg(test)]
mod login_tests {
    use super::*;

    #[test]
    fn login_selects_explicit_saved_or_default_origin() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        assert_eq!(login_api_url(None, &path).unwrap(), DEFAULT_API_URL);
        std::fs::write(
            &path,
            r#"{"api_url":"https://arvumi.example","api_key":null,"access_token":null}"#,
        )
        .unwrap();
        assert_eq!(
            login_api_url(None, &path).unwrap(),
            "https://arvumi.example"
        );
        assert_eq!(
            login_api_url(Some("https://override.example".into()), &path).unwrap(),
            "https://override.example"
        );
        std::fs::write(&path, "invalid JSON").unwrap();
        assert!(login_api_url(None, &path).is_err());
        assert_eq!(
            login_api_url(Some(DEFAULT_API_URL.into()), &path).unwrap(),
            DEFAULT_API_URL
        );
    }
}
