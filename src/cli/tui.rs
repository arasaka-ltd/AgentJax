use std::{io, path::PathBuf, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    DefaultTerminal,
};
use serde_json::json;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

use crate::{
    api::{
        ActorIdentity, ApiMethod, RequestEnvelope, RequestId, RequestMeta, SessionGetResponse,
        SessionListResponse, SessionMessage, SessionSendResponse, StreamEnvelope, StreamPhase,
    },
    cli::request,
    domain::EventType,
    transport::unix::UnixSocketClient,
};

pub async fn run(unix_socket: PathBuf) -> Result<()> {
    let mut tui = TuiApp::new(unix_socket).await?;
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let terminal = ratatui::init();
    let result = tui.run(terminal).await;
    ratatui::restore();
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    result
}

struct TuiApp {
    unix_socket: PathBuf,
    selected_session_index: usize,
    session_id: String,
    sessions: SessionListResponse,
    session: SessionGetResponse,
    input: String,
    status: StatusLine,
    pending_reply: Option<PendingReply>,
    send_in_flight: bool,
    stream_rx: Option<UnboundedReceiver<TuiUpdate>>,
}

#[derive(Clone)]
struct PendingReply {
    turn_id: String,
    stream_id: Option<String>,
    content: String,
    state: PendingReplyState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PendingReplyState {
    Starting,
    Streaming,
    Completed,
    Failed,
}

#[derive(Clone)]
struct StatusLine {
    label: String,
    tone: StatusTone,
}

#[derive(Clone, Copy)]
enum StatusTone {
    Neutral,
    Info,
    Success,
    Warning,
    Error,
}

enum TuiUpdate {
    Started {
        turn_id: String,
        stream_id: Option<String>,
    },
    Chunk {
        text: String,
    },
    End,
    Error {
        message: String,
    },
    Finished(Result<SessionGetResponse, String>),
}

impl TuiApp {
    async fn new(unix_socket: PathBuf) -> Result<Self> {
        let sessions: SessionListResponse =
            request(unix_socket.clone(), ApiMethod::SessionList, json!({})).await?;
        let session_id = sessions
            .items
            .first()
            .map(|item| item.session_id.clone())
            .unwrap_or_else(|| "session.default".into());
        let selected_session_index = sessions
            .items
            .iter()
            .position(|item| item.session_id == session_id)
            .unwrap_or(0);
        let session = fetch_session(unix_socket.clone(), session_id.clone()).await?;

        Ok(Self {
            unix_socket,
            selected_session_index,
            session_id,
            sessions,
            session,
            input: String::new(),
            status: StatusLine::info("Ready"),
            pending_reply: None,
            send_in_flight: false,
            stream_rx: None,
        })
    }

    async fn run(&mut self, mut terminal: DefaultTerminal) -> Result<()> {
        loop {
            self.drain_stream_updates().await?;
            terminal.draw(|frame| self.draw(frame))?;

            if !event::poll(Duration::from_millis(50))? {
                continue;
            }

            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Esc => break,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                    KeyCode::Up => self.select_previous_session().await?,
                    KeyCode::Down => self.select_next_session().await?,
                    KeyCode::Enter => self.submit_input().await?,
                    KeyCode::Backspace => {
                        if !self.send_in_flight {
                            self.input.pop();
                        }
                    }
                    KeyCode::Char(ch) => {
                        if !self.send_in_flight {
                            self.input.push(ch);
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    async fn select_previous_session(&mut self) -> Result<()> {
        if self.send_in_flight || self.sessions.items.is_empty() || self.selected_session_index == 0
        {
            return Ok(());
        }
        self.selected_session_index -= 1;
        self.load_selected_session().await
    }

    async fn select_next_session(&mut self) -> Result<()> {
        if self.send_in_flight || self.sessions.items.is_empty() {
            return Ok(());
        }
        let last_index = self.sessions.items.len().saturating_sub(1);
        if self.selected_session_index >= last_index {
            return Ok(());
        }
        self.selected_session_index += 1;
        self.load_selected_session().await
    }

    async fn load_selected_session(&mut self) -> Result<()> {
        let Some(item) = self.sessions.items.get(self.selected_session_index) else {
            return Ok(());
        };
        self.session_id = item.session_id.clone();
        self.session = fetch_session(self.unix_socket.clone(), self.session_id.clone()).await?;
        self.pending_reply = None;
        self.status = StatusLine::info(format!("Loaded session {}", self.session_id));
        Ok(())
    }

    async fn submit_input(&mut self) -> Result<()> {
        if self.send_in_flight {
            self.status = StatusLine::warning("A reply is already streaming");
            return Ok(());
        }

        let message = self.input.trim().to_string();
        if message.is_empty() {
            return Ok(());
        }

        let session_id = self.session_id.clone();
        let unix_socket = self.unix_socket.clone();
        let actor = ActorIdentity {
            kind: "tui".into(),
            id: "operator.local".into(),
            label: "agentjax-tui".into(),
        };
        let (tx, rx) = unbounded_channel();

        self.input.clear();
        self.send_in_flight = true;
        self.pending_reply = Some(PendingReply {
            turn_id: String::new(),
            stream_id: None,
            content: String::new(),
            state: PendingReplyState::Starting,
        });
        self.stream_rx = Some(rx);
        self.status = StatusLine::info("Sending request to daemon");

        tokio::spawn(async move {
            let client = UnixSocketClient::new(unix_socket.clone(), actor.clone());
            let response = client
                .request_with_streams(
                    RequestEnvelope {
                        id: RequestId(format!("req_{}", chrono::Utc::now().timestamp_millis())),
                        method: ApiMethod::SessionSend,
                        params: json!({
                            "session_id": session_id,
                            "message": SessionMessage::user(message),
                            "stream": true,
                        }),
                        meta: Some(RequestMeta {
                            requester: Some(actor),
                            session_id: Some(session_id.clone()),
                            surface_id: Some("tui.local".into()),
                            ..RequestMeta::default()
                        }),
                    },
                    |stream| {
                        push_stream_update(&tx, stream);
                    },
                )
                .await;

            match response {
                Ok(response) if response.ok => {
                    let Some(result) = response.result else {
                        let _ = tx.send(TuiUpdate::Error {
                            message: "missing response result".into(),
                        });
                        let _ = tx.send(TuiUpdate::Finished(Err("missing response result".into())));
                        return;
                    };

                    match serde_json::from_value::<SessionSendResponse>(result) {
                        Ok(send_response) => {
                            let _ = tx.send(TuiUpdate::Started {
                                turn_id: send_response.turn_id.clone(),
                                stream_id: send_response.stream_id.map(|id| id.0),
                            });
                            match fetch_session(unix_socket, session_id).await {
                                Ok(session) => {
                                    let _ = tx.send(TuiUpdate::Finished(Ok(session)));
                                }
                                Err(error) => {
                                    let _ = tx.send(TuiUpdate::Finished(Err(error.to_string())));
                                }
                            }
                        }
                        Err(error) => {
                            let _ = tx.send(TuiUpdate::Error {
                                message: format!("invalid send response: {error}"),
                            });
                            let _ = tx.send(TuiUpdate::Finished(Err(error.to_string())));
                        }
                    }
                }
                Ok(response) => {
                    let message = response
                        .error
                        .map(|error| error.message)
                        .unwrap_or_else(|| "request failed".into());
                    let _ = tx.send(TuiUpdate::Error {
                        message: message.clone(),
                    });
                    let _ = tx.send(TuiUpdate::Finished(Err(message)));
                }
                Err(error) => {
                    let message = error.to_string();
                    let _ = tx.send(TuiUpdate::Error {
                        message: message.clone(),
                    });
                    let _ = tx.send(TuiUpdate::Finished(Err(message)));
                }
            }
        });

        Ok(())
    }

    async fn drain_stream_updates(&mut self) -> Result<()> {
        let mut disconnected = false;
        let mut buffered = Vec::new();

        if let Some(rx) = self.stream_rx.as_mut() {
            loop {
                match rx.try_recv() {
                    Ok(update) => buffered.push(update),
                    Err(tokio::sync::mpsc::error::TryRecvError::Empty) => break,
                    Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        for update in buffered {
            self.apply_update(update).await?;
        }

        if disconnected {
            self.stream_rx = None;
        }

        Ok(())
    }

    async fn apply_update(&mut self, update: TuiUpdate) -> Result<()> {
        match update {
            TuiUpdate::Started { turn_id, stream_id } => {
                if let Some(pending) = self.pending_reply.as_mut() {
                    pending.turn_id = turn_id;
                    pending.stream_id = stream_id;
                    self.status = StatusLine::info("Assistant is preparing a reply");
                }
            }
            TuiUpdate::Chunk { text } => {
                let pending = self.pending_reply.get_or_insert(PendingReply {
                    turn_id: String::new(),
                    stream_id: None,
                    content: String::new(),
                    state: PendingReplyState::Streaming,
                });
                pending.state = PendingReplyState::Streaming;
                pending.content.push_str(&text);
                self.status = StatusLine::info("Streaming assistant reply");
            }
            TuiUpdate::End => {
                if let Some(pending) = self.pending_reply.as_mut() {
                    pending.state = PendingReplyState::Completed;
                }
                self.status = StatusLine::success("Stream completed");
            }
            TuiUpdate::Error { message } => {
                if let Some(pending) = self.pending_reply.as_mut() {
                    pending.state = PendingReplyState::Failed;
                }
                self.status = StatusLine::error(format!("Stream error: {message}"));
            }
            TuiUpdate::Finished(result) => {
                self.send_in_flight = false;
                self.stream_rx = None;
                match result {
                    Ok(session) => {
                        self.session = session;
                        self.sessions = fetch_sessions(self.unix_socket.clone()).await?;
                        self.selected_session_index = self
                            .sessions
                            .items
                            .iter()
                            .position(|item| item.session_id == self.session_id)
                            .unwrap_or(0);
                        self.status = status_for_session(&self.session);
                        self.pending_reply = None;
                    }
                    Err(error) => {
                        self.status = StatusLine::error(format!("Send failed: {error}"));
                    }
                }
            }
        }

        Ok(())
    }

    fn draw(&self, frame: &mut ratatui::Frame) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(12),
                Constraint::Length(3),
                Constraint::Length(3),
            ])
            .split(frame.area());

        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(30), Constraint::Min(30)])
            .split(layout[1]);
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(8), Constraint::Length(8)])
            .split(body[0]);
        let right = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(12), Constraint::Length(8)])
            .split(body[1]);

        let header = Paragraph::new(format!(
            "AgentJax TUI | session: {} | status: {:?} | mode: {:?}",
            self.session.session.session_id, self.session.session.status, self.session.session.mode
        ))
        .block(Block::default().borders(Borders::ALL).title("Header"));

        let session_items: Vec<ListItem> = self
            .sessions
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut label = item
                    .title
                    .clone()
                    .unwrap_or_else(|| item.session_id.clone());
                if label.len() > 24 {
                    label.truncate(24);
                }
                let prefix = if index == self.selected_session_index {
                    ">"
                } else {
                    " "
                };
                ListItem::new(Line::from(format!(
                    "{prefix} {label}\n  {}",
                    item.session_id
                )))
            })
            .collect();
        let sessions = List::new(session_items)
            .block(Block::default().borders(Borders::ALL).title("Sessions"));

        let details = Paragraph::new(session_details_lines(&self.session, &self.pending_reply))
            .block(Block::default().borders(Borders::ALL).title("Details"))
            .wrap(Wrap { trim: false });

        let chat = Paragraph::new(chat_lines(&self.session, &self.pending_reply))
            .block(Block::default().borders(Borders::ALL).title("Chat"))
            .wrap(Wrap { trim: false });

        let events = Paragraph::new(event_lines(&self.session))
            .block(Block::default().borders(Borders::ALL).title("Events"))
            .wrap(Wrap { trim: false });

        let input = Paragraph::new(self.input.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .title(if self.send_in_flight {
                    "Input (streaming)"
                } else {
                    "Input"
                }),
        );

        let status = Paragraph::new(Line::from(vec![Span::styled(
            self.status.label.as_str(),
            self.status.style(),
        )]))
        .block(Block::default().borders(Borders::ALL).title("Status"));

        frame.render_widget(header, layout[0]);
        frame.render_widget(sessions, left[0]);
        frame.render_widget(details, left[1]);
        frame.render_widget(chat, right[0]);
        frame.render_widget(events, right[1]);
        frame.render_widget(input, layout[2]);
        frame.render_widget(status, layout[3]);
    }
}

impl StatusLine {
    fn info(message: impl Into<String>) -> Self {
        Self {
            label: message.into(),
            tone: StatusTone::Info,
        }
    }

    fn success(message: impl Into<String>) -> Self {
        Self {
            label: message.into(),
            tone: StatusTone::Success,
        }
    }

    fn warning(message: impl Into<String>) -> Self {
        Self {
            label: message.into(),
            tone: StatusTone::Warning,
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            label: message.into(),
            tone: StatusTone::Error,
        }
    }

    fn style(&self) -> Style {
        match self.tone {
            StatusTone::Neutral => Style::default(),
            StatusTone::Info => Style::default().fg(Color::Cyan),
            StatusTone::Success => Style::default().fg(Color::Green),
            StatusTone::Warning => Style::default().fg(Color::Yellow),
            StatusTone::Error => Style::default().fg(Color::Red),
        }
    }
}

fn push_stream_update(tx: &tokio::sync::mpsc::UnboundedSender<TuiUpdate>, stream: &StreamEnvelope) {
    match stream.phase {
        StreamPhase::Start => {}
        StreamPhase::Chunk => {
            if let Some(text) = stream.data.get("text").and_then(|value| value.as_str()) {
                let _ = tx.send(TuiUpdate::Chunk {
                    text: text.to_string(),
                });
            }
        }
        StreamPhase::End => {
            let _ = tx.send(TuiUpdate::End);
        }
        StreamPhase::Error => {
            let _ = tx.send(TuiUpdate::Error {
                message: stream
                    .data
                    .get("message")
                    .and_then(|value| value.as_str())
                    .unwrap_or("stream error")
                    .to_string(),
            });
        }
    }
}

async fn fetch_sessions(unix_socket: PathBuf) -> Result<SessionListResponse> {
    request(unix_socket, ApiMethod::SessionList, json!({})).await
}

async fn fetch_session(unix_socket: PathBuf, session_id: String) -> Result<SessionGetResponse> {
    request(
        unix_socket,
        ApiMethod::SessionGet,
        json!({ "session_id": session_id }),
    )
    .await
}

fn chat_lines(
    session: &SessionGetResponse,
    pending_reply: &Option<PendingReply>,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if session.messages.is_empty() && pending_reply.is_none() {
        return vec![Line::from("(no messages yet)")];
    }

    for message in &session.messages {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}: ", message.display_role()),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(message.content.clone()),
        ]));
        lines.push(Line::from(""));
    }

    if let Some(pending) = pending_reply {
        let label = match pending.state {
            PendingReplyState::Starting => "assistant (starting)",
            PendingReplyState::Streaming => "assistant (streaming)",
            PendingReplyState::Completed => "assistant (completed)",
            PendingReplyState::Failed => "assistant (failed)",
        };
        let content = if pending.content.is_empty() {
            "..."
        } else {
            pending.content.as_str()
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{label}: "),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(content.to_string()),
        ]));
    }

    lines
}

fn session_details_lines(
    session: &SessionGetResponse,
    pending_reply: &Option<PendingReply>,
) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(format!("session_id: {}", session.session.session_id)),
        Line::from(format!("agent_id: {}", session.session.agent_id)),
        Line::from(format!(
            "title: {}",
            session.session.title.clone().unwrap_or_default()
        )),
        Line::from(format!("messages: {}", session.messages.len())),
        Line::from(format!("events: {}", session.events.len())),
        Line::from(format!(
            "last_turn: {}",
            session
                .session
                .last_turn_id
                .clone()
                .unwrap_or_else(|| "-".into())
        )),
    ];

    if let Some(pending) = pending_reply {
        lines.push(Line::from(format!(
            "stream: {}",
            pending
                .stream_id
                .clone()
                .unwrap_or_else(|| "pending".into())
        )));
        lines.push(Line::from(format!("reply_state: {:?}", pending.state)));
    }

    lines
}

fn event_lines(session: &SessionGetResponse) -> Vec<Line<'static>> {
    if session.events.is_empty() {
        return vec![Line::from("(no events yet)")];
    }

    session
        .events
        .iter()
        .rev()
        .take(6)
        .map(|event| Line::from(format!("{:?} @ {}", event.event_type, event.occurred_at)))
        .collect()
}

fn status_for_session(session: &SessionGetResponse) -> StatusLine {
    let recent_events: Vec<_> = session.events.iter().rev().take(8).collect();

    if recent_events.iter().any(|event| {
        event.event_type == EventType::ToolFailed || event.event_type == EventType::TurnFailed
    }) {
        return StatusLine::error("Latest turn failed");
    }
    if recent_events
        .iter()
        .any(|event| event.event_type == EventType::ToolCalled)
    {
        return StatusLine::info("Latest turn used tools");
    }
    if recent_events
        .iter()
        .any(|event| event.event_type == EventType::ToolCompleted)
    {
        return StatusLine::success("Tool call completed");
    }
    if recent_events
        .iter()
        .any(|event| event.event_type == EventType::TurnSucceeded)
    {
        return StatusLine::success("Reply completed");
    }

    StatusLine {
        label: "Ready".into(),
        tone: StatusTone::Neutral,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        status_for_session, PendingReply, PendingReplyState, StatusTone, TuiApp, TuiUpdate,
    };
    use crate::{
        api::{SessionGetResponse, SessionMessage},
        domain::{
            EventSource, EventType, ObjectMeta, RuntimeEvent, Session, SessionMode, SessionStatus,
        },
    };

    #[tokio::test]
    async fn apply_update_accumulates_stream_chunks() {
        let mut app = TuiApp {
            unix_socket: std::path::PathBuf::from("/tmp/agentjax.sock"),
            selected_session_index: 0,
            session_id: "session.default".into(),
            sessions: crate::api::SessionListResponse { items: Vec::new() },
            session: sample_session(),
            input: String::new(),
            status: super::StatusLine::info("Ready"),
            pending_reply: Some(PendingReply {
                turn_id: "turn_1".into(),
                stream_id: Some("str_1".into()),
                content: String::new(),
                state: PendingReplyState::Starting,
            }),
            send_in_flight: true,
            stream_rx: None,
        };

        app.apply_update(TuiUpdate::Chunk {
            text: "hello ".into(),
        })
        .await
        .unwrap();
        app.apply_update(TuiUpdate::Chunk {
            text: "world".into(),
        })
        .await
        .unwrap();
        app.apply_update(TuiUpdate::End).await.unwrap();

        let pending = app.pending_reply.expect("pending reply");
        assert_eq!(pending.content, "hello world");
        assert_eq!(pending.state, PendingReplyState::Completed);
    }

    #[test]
    fn status_detects_tool_activity() {
        let mut session = sample_session();
        session.events.push(sample_event(EventType::ToolCalled));
        let status = status_for_session(&session);
        assert!(matches!(status.tone, StatusTone::Info));
    }

    fn sample_session() -> SessionGetResponse {
        SessionGetResponse {
            session: Session {
                meta: ObjectMeta::new("session.default", "2026-04-10"),
                session_id: "session.default".into(),
                workspace_id: "default-workspace".into(),
                agent_id: "default-agent".into(),
                channel_id: None,
                surface_id: Some("tui.local".into()),
                user_id: Some("operator.local".into()),
                title: Some("Default Session".into()),
                mode: SessionMode::Interactive,
                status: SessionStatus::Active,
                last_turn_id: Some("turn_1".into()),
                current_provider_id: Some("openai-default".into()),
                current_model_id: Some("gpt-4o-mini".into()),
                pending_model_switch: None,
                last_model_switched_at: None,
            },
            messages: vec![SessionMessage::user("hello")],
            events: vec![sample_event(EventType::MessageReceived)],
        }
    }

    fn sample_event(event_type: EventType) -> RuntimeEvent {
        RuntimeEvent {
            event_id: "evt_1".into(),
            event_type,
            occurred_at: chrono::Utc::now(),
            workspace_id: Some("default-workspace".into()),
            agent_id: Some("default-agent".into()),
            session_id: Some("session.default".into()),
            turn_id: Some("turn_1".into()),
            task_id: None,
            plugin_id: None,
            node_id: None,
            source: EventSource::Operator,
            causation_id: None,
            correlation_id: Some("turn_1".into()),
            idempotency_key: None,
            payload: serde_json::json!({}),
            schema_version: "event.v1".into(),
        }
    }
}
