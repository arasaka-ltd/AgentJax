pub mod tui;

use std::path::PathBuf;

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::json;

use crate::{
    api::{
        ActorIdentity, RequestEnvelope, RequestId, RequestMeta, RuntimePingResponse,
        RuntimeStatusResponse, SessionGetResponse, SessionListResponse, SessionMessage,
        SessionSendResponse,
    },
    bootstrap::bootstrap_application,
    config::{ConfigLoader, InitMode, LlmProviderConfig},
    daemon::Daemon,
    plugins::providers::openai::{OpenAiModelCatalog, OpenAiProviderAdapter},
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
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    Provider {
        #[command(subcommand)]
        command: ProviderCommand,
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
enum ConfigCommand {
    Init {
        #[arg(long, value_enum, default_value_t = CliInitMode::Minimal)]
        mode: CliInitMode,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliInitMode {
    Minimal,
    LocalDev,
}

#[derive(Debug, Subcommand)]
enum ProviderCommand {
    List,
    Models {
        #[command(subcommand)]
        command: ProviderModelsCommand,
    },
    Test {
        #[arg(long, default_value = "openai-default")]
        provider_id: String,
        #[arg(long, default_value = "Say hello in one sentence.")]
        prompt: String,
    },
}

#[derive(Debug, Subcommand)]
enum ProviderModelsCommand {
    List {
        #[arg(long, default_value = "openai-default")]
        provider_id: String,
        #[arg(long)]
        refresh: bool,
    },
    Info {
        #[arg(long, default_value = "openai-default")]
        provider_id: String,
        model_id: String,
        #[arg(long)]
        refresh: bool,
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
        Command::Config { command } => run_config(command).await,
        Command::Provider { command } => run_provider(command).await,
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

async fn run_config(command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Init { mode } => {
            ConfigLoader::initialize_default(match mode {
                CliInitMode::Minimal => InitMode::Minimal,
                CliInitMode::LocalDev => InitMode::LocalDev,
            })?;
            println!("initialized config/ runtime/ workspace/");
            Ok(())
        }
    }
}

async fn run_provider(command: ProviderCommand) -> Result<()> {
    let loaded = ConfigLoader::load_default()?;
    match command {
        ProviderCommand::List => {
            #[derive(Serialize)]
            struct ProviderRow {
                provider_id: String,
                kind: String,
                base_url: Option<String>,
                api_key_env: Option<String>,
            }

            let rows = loaded
                .runtime_config
                .agent_runtime
                .llm
                .providers
                .iter()
                .map(|provider| match provider {
                    LlmProviderConfig::OpenAi(config) => ProviderRow {
                        provider_id: config.provider_id.clone(),
                        kind: "openai".into(),
                        base_url: config.base_url.clone(),
                        api_key_env: Some(config.api_key_env.clone()),
                    },
                })
                .collect::<Vec<_>>();
            print_json(&rows)
        }
        ProviderCommand::Models { command } => match command {
            ProviderModelsCommand::List {
                provider_id,
                refresh,
            } => {
                let catalog = load_or_refresh_model_catalog(&loaded, &provider_id, refresh).await?;
                print_json(&catalog.language_models)
            }
            ProviderModelsCommand::Info {
                provider_id,
                model_id,
                refresh,
            } => {
                let catalog = load_or_refresh_model_catalog(&loaded, &provider_id, refresh).await?;
                let model = catalog
                    .language_models
                    .into_iter()
                    .find(|model| model.model_id == model_id)
                    .ok_or_else(|| anyhow!("model not found: {model_id}"))?;
                print_json(&model)
            }
        },
        ProviderCommand::Test {
            provider_id,
            prompt,
        } => {
            let adapter = openai_adapter_from_loaded(&loaded, &provider_id)?;
            let response = adapter
                .prompt_text(&loaded.runtime_config.agent_runtime.default_agent, &prompt)
                .await?;
            #[derive(Serialize)]
            struct TestResponse {
                provider_id: String,
                base_url: String,
                response: String,
            }
            print_json(&TestResponse {
                provider_id,
                base_url: adapter.effective_base_url(),
                response,
            })
        }
    }
}

async fn load_or_refresh_model_catalog(
    loaded: &crate::config::loader::LoadedConfig,
    provider_id: &str,
    refresh: bool,
) -> Result<OpenAiModelCatalog> {
    if !refresh {
        if let Some(snapshot) = loaded
            .runtime_config
            .agent_runtime
            .llm
            .model_catalog
            .providers
            .iter()
            .find(|provider| provider.provider_id == provider_id)
        {
            return Ok(OpenAiModelCatalog {
                provider_id: snapshot.provider_id.clone(),
                base_url: snapshot.base_url.clone().unwrap_or_default(),
                raw_models: Vec::new(),
                language_models: snapshot.language_models.clone(),
            });
        }
    }

    let adapter = openai_adapter_from_loaded(loaded, provider_id)?;
    let catalog = adapter.list_models().await?;
    let default_model = catalog
        .language_models
        .first()
        .map(|model| model.model_id.clone())
        .unwrap_or_else(|| {
            loaded
                .runtime_config
                .agent_runtime
                .default_agent
                .model
                .clone()
        });
    ConfigLoader::write_model_snapshot(
        &loaded.config_root,
        provider_id,
        &default_model,
        adapter.to_snapshot(&catalog),
    )?;
    Ok(catalog)
}

fn openai_adapter_from_loaded(
    loaded: &crate::config::loader::LoadedConfig,
    provider_id: &str,
) -> Result<OpenAiProviderAdapter> {
    loaded
        .runtime_config
        .agent_runtime
        .llm
        .providers
        .iter()
        .find_map(|provider| match provider {
            LlmProviderConfig::OpenAi(config) if config.provider_id == provider_id => {
                Some(OpenAiProviderAdapter::new(config.clone()))
            }
            _ => None,
        })
        .ok_or_else(|| anyhow!("provider not found: {provider_id}"))
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
                    "message": SessionMessage::user(message),
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
