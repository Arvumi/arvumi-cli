use clap::Subcommand;
use serde_json::{Map, Value, json};

#[derive(Subcommand)]
pub enum Action {
    /// Show or update the resource defaults used on the next deployment
    Config {
        #[arg(long)]
        project: Option<String>,
        /// Maximum simultaneous executions per function (one request per worker)
        #[arg(long)]
        max_concurrency: Option<u32>,
        #[arg(long)]
        timeout_seconds: Option<u32>,
        /// CPU limit in millicores; 1000 = one vCPU
        #[arg(long)]
        cpu_millis: Option<u32>,
        #[arg(long, conflicts_with = "reset_memory")]
        memory_mib: Option<u32>,
        /// Use 256 MiB for standalone functions and 512 MiB for Next.js
        #[arg(long)]
        reset_memory: bool,
        /// Temporary storage per worker in MiB (64 default; paid maximum 10240)
        #[arg(long)]
        tmp_size_mib: Option<u32>,
    },
}
fn changes(
    max: Option<u32>,
    timeout: Option<u32>,
    cpu: Option<u32>,
    memory: Option<u32>,
    reset: bool,
    tmp_size: Option<u32>,
) -> Map<String, Value> {
    let mut result = Map::new();
    for (name, value) in [
        ("max_concurrency", max),
        ("timeout_seconds", timeout),
        ("cpu_millis", cpu),
        ("memory_mib", memory),
        ("tmp_size_mib", tmp_size),
    ] {
        if let Some(value) = value {
            result.insert(name.into(), json!(value));
        }
    }
    if reset {
        result.insert("memory_mib".into(), Value::Null);
    }
    result
}
pub async fn run(action: Action) -> eyre::Result<()> {
    let Action::Config {
        project,
        max_concurrency,
        timeout_seconds,
        cpu_millis,
        memory_mib,
        reset_memory,
        tmp_size_mib,
    } = action;
    let project = project
        .or_else(super::read_project_link)
        .ok_or_else(|| eyre::eyre!("Use --project or run 'arvumi link <project-id>' first"))?;
    let config = super::load_config()?;
    let url = format!(
        "{}/api/projects/{project}/function-settings",
        config.api_url.trim_end_matches('/')
    );
    let client = reqwest::Client::new();
    let changes = changes(
        max_concurrency,
        timeout_seconds,
        cpu_millis,
        memory_mib,
        reset_memory,
        tmp_size_mib,
    );
    let request = if changes.is_empty() {
        client.get(url)
    } else {
        client.post(url).json(&changes)
    };
    let response = request
        .header(
            reqwest::header::AUTHORIZATION,
            super::require_auth_header(&config),
        )
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    eyre::ensure!(status.is_success(), "{status}: {body}");
    let value: Value = serde_json::from_str(&body)?;
    println!("{}", serde_json::to_string_pretty(&value)?);
    if !changes.is_empty() {
        println!(
            "Defaults saved. Redeploy to apply them; per-function JSON settings override these defaults."
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_updates_preserve_omission_and_explicit_memory_reset() {
        assert_eq!(
            changes(Some(1), None, None, None, false, None),
            json!({"max_concurrency":1}).as_object().unwrap().clone()
        );
        assert_eq!(
            changes(None, None, None, None, false, Some(10240)),
            json!({"tmp_size_mib":10240}).as_object().unwrap().clone()
        );
        assert_eq!(
            changes(None, None, None, None, true, None),
            json!({"memory_mib":null}).as_object().unwrap().clone()
        );
    }
}
