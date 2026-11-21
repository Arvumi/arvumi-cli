//! Loopback PostgreSQL listener carried by authenticated WSS sessions.
use crate::{ArvumiConfig, load_config, read_project_link, require_auth_header};
use chrono::{DateTime, Utc};
use clap::Subcommand;
use futures_util::{SinkExt, StreamExt};
use reqwest::{Client, Url};
use serde::Deserialize;
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, client::IntoClientRequest},
};
use uuid::Uuid;

// Private wire types: the CLI consumes the API without linking server or SQL code.
// Additive server fields are intentionally ignored by Serde.
#[derive(Deserialize)]
struct RemoteSession {
    id: Uuid,
    #[serde(default)]
    cluster_id: Option<Uuid>,
    database_name: String,
    owner_role: String,
    role_name: String,
    write_access: bool,
    status: String,
    expires_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct CreatedSession {
    #[serde(flatten)]
    session: RemoteSession,
    token: String,
    password: String,
}

#[derive(Subcommand)]
pub enum Action {
    /// Open a local SQL connection; defaults to read-only for 30 minutes
    Connect {
        /// Project name or UUID (defaults to the linked project)
        project: Option<String>,
        #[arg(long)]
        environment: String,
        /// Allow writes and migrations (team owner/admin required)
        #[arg(long)]
        write: bool,
        #[arg(long, default_value_t = 15432)]
        port: u16,
    },
    /// List remote sessions, including their audit status
    Sessions {
        project: Option<String>,
        #[arg(long)]
        environment: String,
    },
    /// Revoke a remote session and terminate its database connections
    Disconnect {
        session_id: uuid::Uuid,
        #[arg(long)]
        project: Option<String>,
    },
}
fn base(config: &ArvumiConfig) -> eyre::Result<Url> {
    let url = Url::parse(&config.api_url)?;
    let local = matches!(
        url.host_str(),
        Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
    );
    eyre::ensure!(
        url.scheme() == "https" || (url.scheme() == "http" && local),
        "Database access requires HTTPS (HTTP only for loopback development)"
    );
    eyre::ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/",
        "Use an API origin without credentials, path, query or fragment"
    );
    Ok(url)
}
async fn response(response: reqwest::Response) -> eyre::Result<reqwest::Response> {
    eyre::ensure!(
        response.status().is_success(),
        "Database access request failed (HTTP {}); check permissions or session status",
        response.status().as_u16()
    );
    Ok(response)
}
async fn project(
    client: &Client,
    base: &Url,
    auth: &str,
    project: Option<String>,
) -> eyre::Result<uuid::Uuid> {
    let value = project
        .or_else(read_project_link)
        .ok_or_else(|| eyre::eyre!("Specify a project or run arvumi link first"))?;
    if let Ok(id) = value.parse() {
        return Ok(id);
    }
    let values: Vec<serde_json::Value> = response(
        client
            .get(base.join("api/projects")?)
            .header("Authorization", auth)
            .send()
            .await?,
    )
    .await?
    .json()
    .await?;
    let matches: Vec<_> = values
        .iter()
        .filter(|p| p["name"].as_str() == Some(&value))
        .collect();
    eyre::ensure!(
        matches.len() == 1,
        "Project name is unknown or ambiguous; use its UUID"
    );
    Ok(matches[0]["id"]
        .as_str()
        .ok_or_else(|| eyre::eyre!("Invalid project response"))?
        .parse()?)
}
async fn revoke(
    client: &Client,
    base: &Url,
    auth: &str,
    project: uuid::Uuid,
    id: uuid::Uuid,
) -> eyre::Result<()> {
    response(
        client
            .delete(base.join(&format!("api/projects/{project}/database/sessions/{id}"))?)
            .header("Authorization", auth)
            .send()
            .await?,
    )
    .await?;
    Ok(())
}
pub async fn run(action: Action) -> eyre::Result<()> {
    let config = load_config()?;
    let base = base(&config)?;
    let auth = require_auth_header(&config);
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(40))
        .build()?;
    match action {
        Action::Sessions {
            project: p,
            environment,
        } => {
            let p = project(&client, &base, &auth, p).await?;
            let records: Vec<RemoteSession> = response(
                client
                    .get(base.join(&format!("api/projects/{p}/database/sessions"))?)
                    .header("Authorization", &auth)
                    .query(&[("environment", environment)])
                    .send()
                    .await?,
            )
            .await?
            .json()
            .await?;
            for s in records {
                println!(
                    "{}  {}  {}  expires {}",
                    s.id,
                    if s.write_access { "write" } else { "read-only" },
                    s.status,
                    s.expires_at
                );
            }
        }
        Action::Disconnect {
            session_id,
            project: p,
        } => {
            let p = project(&client, &base, &auth, p).await?;
            revoke(&client, &base, &auth, p, session_id).await?;
            println!("Session {session_id} revoked.");
        }
        Action::Connect {
            project: p,
            environment,
            write,
            port,
        } => {
            let p = project(&client, &base, &auth, p).await?;
            // Bind before issuing a credential; a busy port cannot leak a live session.
            let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
            let session: CreatedSession = response(
                client
                    .post(base.join(&format!("api/projects/{p}/database/sessions"))?)
                    .timeout(Duration::from_secs(300))
                    .header("Authorization", &auth)
                    .json(&serde_json::json!({"environment":environment,"write":write}))
                    .send()
                    .await?,
            )
            .await?
            .json()
            .await?;
            let mut ws = base.join(&format!(
                "api/database-sessions/{}/tunnel",
                session.session.id
            ))?;
            ws.set_scheme(if base.scheme() == "https" {
                "wss"
            } else {
                "ws"
            })
            .map_err(|_| eyre::eyre!("Invalid WebSocket origin"))?;
            let local_port = listener.local_addr()?.port();
            println!(
                "Session {} ({}) expires {}",
                session.session.id,
                if write { "write" } else { "read-only" },
                session.session.expires_at
            );
            println!(
                "psql 'postgresql://{}:{}@127.0.0.1:{}/{}?sslmode=disable'",
                session.session.role_name,
                session.password,
                local_port,
                session.session.database_name
            );
            println!(
                "Local connections use an encrypted network tunnel. Press Ctrl-C to disconnect."
            );
            if session.session.cluster_id.is_some() {
                println!(
                    "Free team databases sleep after 15 idle minutes; dedicated paid databases stay running. Free cold wakes can take up to two minutes. Query and transaction timeouts depend on the database tier and configured service limits."
                );
            }
            if write {
                println!(
                    "For migrations with stable object ownership: SET ROLE \"{}\";",
                    session.session.owner_role
                );
            }
            let lifetime = (session.session.expires_at - chrono::Utc::now())
                .to_std()
                .unwrap_or_default();
            let end = tokio::time::sleep(lifetime);
            tokio::pin!(end);
            let semaphore = Arc::new(Semaphore::new(4));
            let mut tasks = JoinSet::new();
            loop {
                tokio::select! {
                    _=tokio::signal::ctrl_c()=>break,
                    _=&mut end=>{eprintln!("Database session expired.");break;},
                    Some(_)=tasks.join_next()=>{},
                    accepted=listener.accept()=>{
                        let Ok((socket,_))=accepted else {break;};
                        let Ok(permit)=semaphore.clone().try_acquire_owned() else {drop(socket);continue;};
                        let ws=ws.clone();let token=session.token.clone();
                        tasks.spawn(async move {let _permit=permit;
                            if connection(socket,ws,token).await.is_err() {eprintln!("Database connection closed; check session status or reconnect.");}
                        });
                    }
                }
            }
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
            drop(listener);
            if !matches!(
                tokio::time::timeout(
                    Duration::from_secs(10),
                    revoke(&client, &base, &auth, p, session.session.id)
                )
                .await,
                Ok(Ok(()))
            ) {
                eprintln!(
                    "Local tunnel stopped. Remote revocation could not be confirmed; revoke with arvumi db disconnect {} --project {p}. Automatic expiry remains {}.",
                    session.session.id, session.session.expires_at
                );
            }
        }
    }
    Ok(())
}
async fn connection(socket: TcpStream, url: Url, token: String) -> eyre::Result<()> {
    let mut request = url.as_str().into_client_request()?;
    request
        .headers_mut()
        .insert("Authorization", format!("Bearer {token}").parse()?);
    let (ws, _) = tokio::time::timeout(
        Duration::from_secs(10),
        connect_async_with_config(
            request,
            Some(tokio_tungstenite::tungstenite::protocol::WebSocketConfig {
                max_message_size: Some(65536),
                max_frame_size: Some(65536),
                max_write_buffer_size: 131072,
                write_buffer_size: 0,
                ..Default::default()
            }),
            false,
        ),
    )
    .await??;
    let (mut send, mut receive) = ws.split();
    let (mut read, mut write) = socket.into_split();
    let upstream = async {
        let mut buf = vec![0; 16384];
        loop {
            let n = read.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            send.send(Message::Binary(buf[..n].to_vec())).await?;
        }
        Ok::<(), eyre::Report>(())
    };
    let downstream = async {
        while let Some(message) = receive.next().await {
            match message? {
                Message::Binary(bytes) => write.write_all(&bytes).await?,
                Message::Close(_) => break,
                Message::Ping(_) | Message::Pong(_) => {}
                _ => {
                    eyre::bail!("Unexpected tunnel frame");
                }
            }
        }
        Ok::<(), eyre::Report>(())
    };
    tokio::select! {r=upstream=>r,r=downstream=>r}
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_secure_origin() {
        for url in ["https://app.arvumi.com", "http://127.0.0.1:3000"] {
            assert!(
                base(&ArvumiConfig {
                    api_url: url.into(),
                    api_key: None,
                    access_token: None
                })
                .is_ok()
            );
        }
        for url in [
            "http://example.com",
            "https://example.com/path",
            "https://user:pass@example.com",
            "https://example.com?token=x",
        ] {
            assert!(
                base(&ArvumiConfig {
                    api_url: url.into(),
                    api_key: None,
                    access_token: None
                })
                .is_err()
            );
        }
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use serde_json::{Value, json};

    // Matches the public serialization of core::remote_database::RemoteSession.
    // api_key_id and token_hash are deliberately never part of the wire response.
    fn session() -> Value {
        json!({
            "id": "7d06d5b1-f719-4f76-914b-7d931d906d43",
            "project_id": "795b8c8f-110a-415f-bebd-0b7ae59822fc",
            "database_id": "c8827fce-0a26-4131-99c0-50183514694c",
            "cluster_id": "055a9b37-0e21-4b26-9fbf-3d90deeff606",
            "environment": "production",
            "database_name": "arv_c8827fce0a26413199c050183514694c",
            "owner_role": "arv_c8827fce0a26413199c050183514694c",
            "role_name": "arv_remote_7d06d5b1f7194f76914b7d931d906d43",
            "user_id": "ed94f8eb-229e-4623-8f48-d24f19b54c80",
            "write_access": false,
            "status": "active",
            "created_at": "2026-11-19T12:00:00Z",
            "expires_at": "2026-11-19T12:30:00Z",
            "connected_at": null, "revoked_at": null, "cleaned_at": null,
            "reason": null,
            "future_server_field": {"ignored": true}
        })
    }

    #[test]
    fn list_accepts_server_fields_and_optional_cluster() {
        let mut value = session();
        let rows: Vec<RemoteSession> = serde_json::from_value(json!([value])).unwrap();
        assert_eq!(
            rows[0].id.to_string(),
            "7d06d5b1-f719-4f76-914b-7d931d906d43"
        );
        assert!(!rows[0].write_access);
        assert_eq!(rows[0].status, "active");
        assert!(rows[0].cluster_id.is_some());
        value["cluster_id"] = Value::Null;
        assert!(
            serde_json::from_value::<RemoteSession>(value.clone())
                .unwrap()
                .cluster_id
                .is_none()
        );
        value.as_object_mut().unwrap().remove("cluster_id");
        assert!(
            serde_json::from_value::<RemoteSession>(value)
                .unwrap()
                .cluster_id
                .is_none()
        );
    }

    #[test]
    fn creation_deserializes_flattened_credentials_and_rejects_incomplete_response() {
        let mut value = session();
        value["token"] = json!("dummy-session-token");
        value["password"] = json!("dummy-password");
        let created: CreatedSession = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(created.token, "dummy-session-token");
        assert_eq!(created.password, "dummy-password");
        assert_eq!(created.session.database_name, created.session.owner_role);
        assert!(created.session.role_name.starts_with("arv_remote_"));
        assert_eq!(
            created.session.expires_at.to_rfc3339(),
            "2026-11-19T12:30:00+00:00"
        );
        for field in ["id", "expires_at", "role_name", "token", "password"] {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<CreatedSession>(missing).is_err(),
                "missing {field}"
            );
        }
    }
}
