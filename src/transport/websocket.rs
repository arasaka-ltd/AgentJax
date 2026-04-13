use anyhow::{anyhow, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

use crate::{
    api::{ApiError, ApiErrorCode, ApiErrorEnvelope, ClientEnvelope, ServerEnvelope},
    daemon::{Daemon, Dispatch, API_VERSION},
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
        write_dispatch(&mut socket, dispatch).await?;
    }

    Ok(())
}

async fn write_dispatch(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    dispatch: Dispatch,
) -> Result<()> {
    write_server_message(socket, &dispatch.response).await?;
    for followup in dispatch.followups {
        write_server_message(socket, &followup).await?;
    }
    if let Some(mut live_stream) = dispatch.live_stream {
        while let Some(envelope) = live_stream.recv().await {
            write_server_message(socket, &envelope).await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    use crate::{
        api::SessionSendResponse,
        api::{
            ActorIdentity, ClientEnvelope, HelloEnvelope, RequestEnvelope, RequestId,
            ServerEnvelope, StreamPhase,
        },
        test_support::TestHarness,
    };

    #[tokio::test]
    async fn websocket_connection_forwards_live_streams() {
        let harness = TestHarness::new("ws-live-stream");
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind websocket test listener");
        let addr = listener
            .local_addr()
            .expect("failed to read websocket listener address");
        let daemon = harness.daemon.clone();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept failed");
            handle_connection(stream, daemon)
                .await
                .expect("websocket handler failed");
        });

        let (mut socket, _) = connect_async(format!("ws://{addr}"))
            .await
            .expect("failed to connect websocket client");

        socket
            .send(Message::Text(
                serde_json::to_string(&ClientEnvelope::Hello(HelloEnvelope {
                    api_version: API_VERSION.into(),
                    client: ActorIdentity {
                        kind: "cli".into(),
                        id: "operator.local".into(),
                        label: "test-client".into(),
                    },
                    capabilities: vec!["request_response".into(), "streams".into()],
                }))
                .expect("failed to encode hello")
                .into(),
            ))
            .await
            .expect("failed to send websocket hello");

        match read_server_envelope(&mut socket).await {
            ServerEnvelope::HelloAck(ack) => assert!(ack.ok),
            other => panic!("expected hello_ack, got {other:?}"),
        }

        socket
            .send(Message::Text(
                serde_json::to_string(&ClientEnvelope::Request(RequestEnvelope {
                    id: RequestId("req_ws_stream".into()),
                    method: crate::api::ApiMethod::SessionSend,
                    params: json!({
                        "session_id": "session.default",
                        "message": crate::api::SessionMessage::user("ping"),
                        "stream": true,
                    }),
                    meta: None,
                }))
                .expect("failed to encode websocket request")
                .into(),
            ))
            .await
            .expect("failed to send websocket request");

        let response = match read_server_envelope(&mut socket).await {
            ServerEnvelope::Response(response) => response,
            other => panic!("expected response, got {other:?}"),
        };
        assert!(response.ok);
        let send_response: SessionSendResponse =
            serde_json::from_value(response.result.expect("missing session.send result"))
                .expect("failed to decode session.send response");
        let stream_id = send_response
            .stream_id
            .expect("streaming response missing stream_id")
            .0;

        let mut phases = Vec::new();
        let mut events = Vec::new();
        loop {
            match read_server_envelope(&mut socket).await {
                ServerEnvelope::Stream(stream) => {
                    assert_eq!(stream.stream_id.0, stream_id);
                    phases.push(stream.phase.clone());
                    events.push(stream.event.clone());
                    if matches!(stream.phase, StreamPhase::End) {
                        break;
                    }
                }
                other => panic!("expected stream envelope, got {other:?}"),
            }
        }

        assert_eq!(phases.first(), Some(&StreamPhase::Start));
        assert_eq!(phases.last(), Some(&StreamPhase::End));
        assert!(events.iter().any(|event| event == "turn.started"));
        assert!(events.iter().any(|event| event == "stream.completed"));

        socket.close(None).await.expect("failed to close websocket");
        server.await.expect("websocket server task failed");
    }

    async fn read_server_envelope(
        socket: &mut tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    ) -> ServerEnvelope {
        loop {
            match socket
                .next()
                .await
                .expect("websocket closed unexpectedly")
                .expect("websocket read failed")
            {
                Message::Text(text) => {
                    return serde_json::from_str(&text).expect("failed to decode server envelope")
                }
                Message::Ping(_) | Message::Pong(_) => continue,
                Message::Close(frame) => panic!("unexpected websocket close: {frame:?}"),
                Message::Binary(_) | Message::Frame(_) => {
                    panic!("unexpected non-text websocket message")
                }
            }
        }
    }
}
