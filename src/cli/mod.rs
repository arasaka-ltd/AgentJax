pub mod tui;

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use serde::Serialize;
use serde_json::json;

use crate::{
    api::{
        ActorIdentity, RequestEnvelope, RequestId, RequestMeta, RuntimePingResponse,
        RuntimeStatusResponse, SessionGetResponse, SessionListResponse, SessionMessage,
        SessionSendResponse,
    },
    bootstrap::bootstrap_application,
    daemon::Daemon,
    transport::{unix::UnixSocketClient, unix::UnixSocketServer, websocket::WebSocketServer},
};

#[derive(Debug, Parser)]
#[command(name = "agentjax")]
#[command(about = "AgentJax daemon and client entrypoint")]
struct Cli {
    #[arg(long, default_value = "runtime/run/daemon.sock")]
    unix_socket: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Daemon {
        #[arg(long, default_value = "127.0.0.1:4080")]
        ws_bind: String,
        #[arg(long)]
        no_ws: bool,
    },
    Ping,
    Status,
    Tui,
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    List,
    Get {
        session_id: String,
    },
    Send {
        session_id: String,
        message: String,
        #[arg(long, default_value_t = false)]
        stream: bool,
    },
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Daemon { ws_bind, no_ws } => run_daemon(cli.unix_socket, ws_bind, no_ws).await,
        Command::Ping => {
            let response: RuntimePingResponse = request(
                cli.unix_socket,
                crate::api::ApiMethod::RuntimePing,
                json!({}),
            )
            .await?;
            print_json(&response)
        }
        Command::Status => {
            let response: RuntimeStatusResponse = request(
                cli.unix_socket,
                crate::api::ApiMethod::RuntimeStatus,
                json!({}),
            )
            .await?;
            print_json(&response)
        }
        Command::Tui => tui::run(cli.unix_socket).await,
        Command::Session { command } => match command {
            SessionCommand::List => {
                let response: SessionListResponse = request(
                    cli.unix_socket,
                    crate::api::ApiMethod::SessionList,
                    json!({}),
                )
                .await?;
                print_json(&response)
            }
            SessionCommand::Get { session_id } => {
                let response: SessionGetResponse = request(
                    cli.unix_socket,
                    crate::api::ApiMethod::SessionGet,
                    json!({ "session_id": session_id }),
                )
                .await?;
                print_json(&response)
            }
            SessionCommand::Send {
                session_id,
                message,
                stream,
            } => {
                let response: SessionSendResponse =
                    session_send(cli.unix_socket, session_id, message, stream).await?;
                print_json(&response)
            }
        },
    }
}

async fn run_daemon(unix_socket: PathBuf, ws_bind: String, no_ws: bool) -> Result<()> {
    let app = bootstrap_application()?;
    let daemon = Daemon::new(app);
    let unix_server = UnixSocketServer::new(daemon.clone(), unix_socket);

    if no_ws {
        unix_server.run().await
    } else {
        let ws_server = WebSocketServer::new(daemon, ws_bind);
        tokio::try_join!(unix_server.run(), ws_server.run())?;
        Ok(())
    }
}

pub async fn request<T>(
    unix_socket: PathBuf,
    method: crate::api::ApiMethod,
    params: serde_json::Value,
) -> Result<T>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let client = UnixSocketClient::new(
        unix_socket,
        ActorIdentity {
            kind: "cli".into(),
            id: "operator.local".into(),
            label: "agentjax-cli".into(),
        },
    );
    let response = client
        .request(RequestEnvelope {
            id: RequestId(format!("req_{}", chrono::Utc::now().timestamp_millis())),
            method,
            params,
            meta: Some(RequestMeta {
                requester: Some(ActorIdentity {
                    kind: "cli".into(),
                    id: "operator.local".into(),
                    label: "agentjax-cli".into(),
                }),
                surface_id: Some("cli.local".into()),
                ..RequestMeta::default()
            }),
        })
        .await?;

    if response.ok {
        let result = response
            .result
            .ok_or_else(|| anyhow!("missing response result"))?;
        Ok(serde_json::from_value(result)?)
    } else {
        let error = response
            .error
            .ok_or_else(|| anyhow!("missing response error"))?;
        Err(anyhow!(error.message))
    }
}

pub async fn session_send(
    unix_socket: PathBuf,
    session_id: String,
    message: String,
    stream: bool,
) -> Result<SessionSendResponse> {
    let actor = ActorIdentity {
        kind: "cli".into(),
        id: "operator.local".into(),
        label: "agentjax-cli".into(),
    };
    let client = UnixSocketClient::new(unix_socket, actor.clone());
    let response = client
        .request_with_streams(
            RequestEnvelope {
                id: RequestId(format!("req_{}", chrono::Utc::now().timestamp_millis())),
                method: crate::api::ApiMethod::SessionSend,
                params: json!({
                    "session_id": session_id,
                    "message": SessionMessage {
                        role: "user".into(),
                        content: message,
                    },
                    "stream": stream,
                }),
                meta: Some(RequestMeta {
                    requester: Some(actor),
                    surface_id: Some("cli.local".into()),
                    ..RequestMeta::default()
                }),
            },
            |stream_envelope| {
                if let Some(text) = stream_envelope
                    .data
                    .get("text")
                    .and_then(|value| value.as_str())
                {
                    print!("{text}");
                }
                if matches!(stream_envelope.phase, crate::api::StreamPhase::End) {
                    println!();
                }
            },
        )
        .await?;

    if response.ok {
        let result = response
            .result
            .ok_or_else(|| anyhow!("missing response result"))?;
        Ok(serde_json::from_value(result)?)
    } else {
        let error = response
            .error
            .ok_or_else(|| anyhow!("missing response error"))?;
        Err(anyhow!(error.message))
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
