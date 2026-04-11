use std::{io, os::unix::fs::FileTypeExt, path::PathBuf};

use anyhow::{anyhow, Context, Result};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

use crate::{
    api::{
        ActorIdentity, ApiError, ApiErrorCode, ApiErrorEnvelope, ClientEnvelope, RequestEnvelope,
        ResponseEnvelope, ServerEnvelope,
    },
    daemon::{Daemon, API_VERSION},
};

#[derive(Clone)]
pub struct UnixSocketServer {
    daemon: Daemon,
    socket_path: PathBuf,
}

impl UnixSocketServer {
    pub fn new(daemon: Daemon, socket_path: impl Into<PathBuf>) -> Self {
        Self {
            daemon,
            socket_path: socket_path.into(),
        }
    }

    pub async fn run(self) -> Result<()> {
        if let Some(parent) = self.socket_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }

        if matches!(tokio::fs::metadata(&self.socket_path).await, Ok(metadata) if metadata.file_type().is_socket())
        {
            let _ = tokio::fs::remove_file(&self.socket_path).await;
        }

        let listener = UnixListener::bind(&self.socket_path)
            .with_context(|| format!("failed to bind {}", self.socket_path.display()))?;

        loop {
            let (stream, _) = listener.accept().await?;
            let daemon = self.daemon.clone();
            tokio::spawn(async move {
                if let Err(error) = handle_connection(stream, daemon).await {
                    eprintln!("unix socket connection failed: {error}");
                }
            });
        }
    }
}

pub struct UnixSocketClient {
    socket_path: PathBuf,
    actor: ActorIdentity,
}

impl UnixSocketClient {
    pub fn new(socket_path: impl Into<PathBuf>, actor: ActorIdentity) -> Self {
        Self {
            socket_path: socket_path.into(),
            actor,
        }
    }

    pub async fn request(&self, request: RequestEnvelope) -> Result<ResponseEnvelope> {
        self.request_with_streams(request, |_| {}).await
    }

    pub async fn request_with_streams<F>(
        &self,
        request: RequestEnvelope,
        mut on_stream: F,
    ) -> Result<ResponseEnvelope>
    where
        F: FnMut(&crate::api::StreamEnvelope),
    {
        let stream = UnixStream::connect(&self.socket_path)
            .await
            .with_context(|| format!("failed to connect {}", self.socket_path.display()))?;
        let (read_half, mut write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);

        let hello = ClientEnvelope::Hello(crate::api::HelloEnvelope {
            api_version: API_VERSION.into(),
            client: self.actor.clone(),
            capabilities: vec!["request_response".into()],
        });
        write_json_line(&mut write_half, &hello).await?;

        let ack = read_json_line::<ServerEnvelope>(&mut reader).await?;
        match ack {
            ServerEnvelope::HelloAck(ack) if ack.ok => {}
            ServerEnvelope::Error(error) => return Err(anyhow!(error.error.message)),
            other => return Err(anyhow!("unexpected handshake response: {other:?}")),
        }

        write_json_line(&mut write_half, &ClientEnvelope::Request(request)).await?;
        loop {
            match read_json_line::<ServerEnvelope>(&mut reader).await? {
                ServerEnvelope::Response(response) => {
                    let pending_stream_id = response
                        .result
                        .as_ref()
                        .and_then(|result| result.get("stream_id"))
                        .and_then(|value| value.as_str())
                        .map(str::to_owned);
                    if pending_stream_id.is_none() {
                        return Ok(response);
                    }

                    loop {
                        match read_json_line::<ServerEnvelope>(&mut reader).await? {
                            ServerEnvelope::Stream(stream) => {
                                let is_target = pending_stream_id.as_deref()
                                    == Some(stream.stream_id.0.as_str());
                                let is_end = matches!(stream.phase, crate::api::StreamPhase::End);
                                on_stream(&stream);
                                if is_target && is_end {
                                    return Ok(response);
                                }
                            }
                            ServerEnvelope::Event(_) => continue,
                            ServerEnvelope::Error(error) => {
                                return Err(anyhow!(error.error.message));
                            }
                            ServerEnvelope::Response(_) | ServerEnvelope::HelloAck(_) => {
                                return Err(anyhow!(
                                    "unexpected envelope while waiting for stream"
                                ));
                            }
                        }
                    }
                }
                ServerEnvelope::Error(error) => return Err(anyhow!(error.error.message)),
                ServerEnvelope::Event(_) => continue,
                ServerEnvelope::Stream(stream) => {
                    on_stream(&stream);
                    continue;
                }
                ServerEnvelope::HelloAck(_) => {
                    return Err(anyhow!("unexpected hello_ack after handshake"));
                }
            }
        }
    }
}

async fn handle_connection(stream: UnixStream, daemon: Daemon) -> Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let hello = read_json_line::<ClientEnvelope>(&mut reader).await?;

    match hello {
        ClientEnvelope::Hello(hello) if hello.api_version == API_VERSION => {
            let ack = ServerEnvelope::HelloAck(daemon.hello_ack(daemon.connection_id()));
            write_json_line(&mut write_half, &ack).await?;
        }
        ClientEnvelope::Hello(_) => {
            let error = protocol_error(ApiErrorCode::UnsupportedVersion, "unsupported api version");
            write_json_line(&mut write_half, &ServerEnvelope::Error(error)).await?;
            return Ok(());
        }
        ClientEnvelope::Request(_) => {
            let error = protocol_error(ApiErrorCode::ProtocolViolation, "missing hello handshake");
            write_json_line(&mut write_half, &ServerEnvelope::Error(error)).await?;
            return Ok(());
        }
    }

    loop {
        let line = match read_next_line(&mut reader).await? {
            Some(line) => line,
            None => return Ok(()),
        };
        let envelope: ClientEnvelope = match serde_json::from_str(&line) {
            Ok(envelope) => envelope,
            Err(error) => {
                let error = protocol_error(
                    ApiErrorCode::ProtocolViolation,
                    format!("invalid client envelope: {error}"),
                );
                write_json_line(&mut write_half, &ServerEnvelope::Error(error)).await?;
                continue;
            }
        };

        let dispatch = daemon
            .handle_client_envelope(envelope)
            .await
            .unwrap_or_else(|error| {
                crate::daemon::Dispatch::single(ServerEnvelope::Error(ApiErrorEnvelope::new(error)))
            });
        write_json_line(&mut write_half, &dispatch.response).await?;
        for followup in dispatch.followups {
            write_json_line(&mut write_half, &followup).await?;
        }
    }
}

async fn read_json_line<T>(reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>) -> Result<T>
where
    T: for<'de> serde::Deserialize<'de>,
{
    let line = read_next_line(reader)
        .await?
        .ok_or_else(|| anyhow!("connection closed"))?;
    serde_json::from_str(&line).context("failed to decode json line")
}

async fn read_next_line(
    reader: &mut BufReader<tokio::net::unix::OwnedReadHalf>,
) -> io::Result<Option<String>> {
    let mut line = String::new();
    let bytes = reader.read_line(&mut line).await?;
    if bytes == 0 {
        return Ok(None);
    }
    Ok(Some(line))
}

async fn write_json_line<T>(writer: &mut tokio::net::unix::OwnedWriteHalf, value: &T) -> Result<()>
where
    T: serde::Serialize,
{
    let mut line = serde_json::to_vec(value)?;
    line.push(b'\n');
    writer.write_all(&line).await?;
    writer.flush().await?;
    Ok(())
}

fn protocol_error(code: ApiErrorCode, message: impl Into<String>) -> ApiErrorEnvelope {
    ApiErrorEnvelope::new(ApiError::new(code, message, false))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::{
        api::{ActorIdentity, ApiMethod, RequestEnvelope, RequestId, StreamPhase},
        app::Application,
        config::{
            ConfigRoot, RuntimeConfig, WorkspaceConfig, WorkspaceIdentityPack, WorkspacePaths,
        },
        daemon::Daemon,
    };

    use super::{UnixSocketClient, UnixSocketServer};

    #[tokio::test]
    async fn unix_socket_streams_logs_tail_followups() {
        let root = temp_path("unix-socket-streams");
        let workspace_root = root.join("workspace");
        fs::create_dir_all(&workspace_root).unwrap();

        let socket_path =
            std::path::PathBuf::from(format!("/tmp/agentjax-transport-{}.sock", unique_suffix()));
        let daemon = Daemon::new(
            Application::new(
                ConfigRoot::new(root.join("config")),
                test_runtime_config(&root, &workspace_root),
                test_identity(&workspace_root),
            )
            .unwrap(),
        )
        .unwrap();
        let server = tokio::spawn(UnixSocketServer::new(daemon, &socket_path).run());
        wait_for_socket(&socket_path).await;

        let client = UnixSocketClient::new(
            socket_path.clone(),
            ActorIdentity {
                kind: "test".into(),
                id: "transport.unix".into(),
                label: "transport-test".into(),
            },
        );
        let mut phases = Vec::new();
        let response = client
            .request_with_streams(
                RequestEnvelope {
                    id: RequestId("req_logs_stream".into()),
                    method: ApiMethod::LogsTail,
                    params: serde_json::json!({ "stream": true }),
                    meta: None,
                },
                |stream| phases.push(stream.phase.clone()),
            )
            .await
            .unwrap();

        assert!(response.ok);
        assert!(phases.len() >= 2);
        assert!(matches!(phases.first(), Some(StreamPhase::Start)));
        assert!(matches!(phases.last(), Some(StreamPhase::End)));

        server.abort();
        let _ = fs::remove_dir_all(root);
    }

    fn test_runtime_config(root: &Path, workspace_root: &Path) -> RuntimeConfig {
        RuntimeConfig::new(
            "agentjax-test",
            crate::config::RuntimePaths::new(root.join("runtime")),
            WorkspaceConfig::new("default-workspace", WorkspacePaths::new(workspace_root)),
        )
    }

    fn test_identity(workspace_root: &Path) -> WorkspaceIdentityPack {
        WorkspaceIdentityPack {
            workspace_id: "default-workspace".into(),
            agent: doc(workspace_root.join("AGENT.md"), ""),
            soul: doc(workspace_root.join("SOUL.md"), ""),
            user: doc(workspace_root.join("USER.md"), ""),
            memory: doc(workspace_root.join("MEMORY.md"), ""),
            mission: doc(workspace_root.join("MISSION.md"), ""),
            rules: doc(workspace_root.join("RULES.md"), ""),
            router: doc(workspace_root.join("ROUTER.md"), ""),
        }
    }

    fn doc(path: PathBuf, content: &str) -> crate::config::WorkspaceDocument {
        crate::config::WorkspaceDocument {
            path,
            content: content.into(),
        }
    }

    async fn wait_for_socket(path: &Path) {
        for _ in 0..50 {
            if path.exists() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("socket was not created: {}", path.display());
    }

    fn temp_path(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!("agentjax-{prefix}-{}", unique_suffix()))
    }

    fn unique_suffix() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    }
}
