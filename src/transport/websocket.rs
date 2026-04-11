use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

use crate::{
    api::{ApiError, ApiErrorCode, ApiErrorEnvelope, ClientEnvelope, ServerEnvelope},
    daemon::{Daemon, API_VERSION},
};

#[derive(Clone)]
pub struct WebSocketServer {
    daemon: Daemon,
    bind_addr: String,
}

impl WebSocketServer {
    pub fn new(daemon: Daemon, bind_addr: impl Into<String>) -> Self {
        Self {
            daemon,
            bind_addr: bind_addr.into(),
        }
    }

    pub async fn run(self) -> Result<()> {
        let listener = TcpListener::bind(&self.bind_addr).await?;
        loop {
            let (stream, _) = listener.accept().await?;
            let daemon = self.daemon.clone();
            tokio::spawn(async move {
                if let Err(error) = handle_connection(stream, daemon).await {
                    eprintln!("websocket connection failed: {error}");
                }
            });
        }
    }
}

async fn handle_connection(stream: tokio::net::TcpStream, daemon: Daemon) -> Result<()> {
    let mut socket = accept_async(stream).await?;
    let hello = read_client_message(&mut socket)
        .await?
        .ok_or_else(|| anyhow!("websocket closed before hello"))?;

    match hello {
        ClientEnvelope::Hello(hello) if hello.api_version == API_VERSION => {
            write_server_message(
                &mut socket,
                &ServerEnvelope::HelloAck(daemon.hello_ack(daemon.connection_id())),
            )
            .await?;
        }
        ClientEnvelope::Hello(_) => {
            write_server_message(
                &mut socket,
                &ServerEnvelope::Error(protocol_error(
                    ApiErrorCode::UnsupportedVersion,
                    "unsupported api version",
                )),
            )
            .await?;
            return Ok(());
        }
        ClientEnvelope::Request(_) => {
            write_server_message(
                &mut socket,
                &ServerEnvelope::Error(protocol_error(
                    ApiErrorCode::ProtocolViolation,
                    "missing hello handshake",
                )),
            )
            .await?;
            return Ok(());
        }
    }

    while let Some(envelope) = read_client_message(&mut socket).await? {
        let dispatch = daemon
            .handle_client_envelope(envelope)
            .await
            .unwrap_or_else(|error| {
                crate::daemon::Dispatch::single(ServerEnvelope::Error(ApiErrorEnvelope::new(error)))
            });
        write_server_message(&mut socket, &dispatch.response).await?;
        for followup in dispatch.followups {
            write_server_message(&mut socket, &followup).await?;
        }
    }

    Ok(())
}

async fn read_client_message(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
) -> Result<Option<ClientEnvelope>> {
    while let Some(message) = socket.next().await {
        match message? {
            Message::Text(text) => {
                let envelope = serde_json::from_str(&text)?;
                return Ok(Some(envelope));
            }
            Message::Binary(_) => return Err(anyhow!("binary websocket frames are not supported")),
            Message::Ping(payload) => socket.send(Message::Pong(payload)).await?,
            Message::Pong(_) => {}
            Message::Close(_) => return Ok(None),
            Message::Frame(_) => {}
        }
    }

    Ok(None)
}

async fn write_server_message(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    envelope: &ServerEnvelope,
) -> Result<()> {
    socket
        .send(Message::Text(serde_json::to_string(envelope)?))
        .await?;
    Ok(())
}

fn protocol_error(code: ApiErrorCode, message: impl Into<String>) -> ApiErrorEnvelope {
    ApiErrorEnvelope::new(ApiError::new(code, message, false))
}
