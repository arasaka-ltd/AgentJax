use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Context, Result};
use crossterm::{
    cursor::{self, MoveToColumn},
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    style::{Attribute, Color, Stylize},
    terminal::{self, Clear, ClearType},
};
use serde_json::json;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::{
    api::{
        ActorIdentity, ApiMethod, RequestEnvelope, RequestId, RequestMeta, SessionGetResponse,
        SessionListResponse, SessionMessage, SessionMessageKind, SessionSendResponse,
        StreamEnvelope, StreamPhase,
    },
    cli::request,
    transport::unix::UnixSocketClient,
};

const INITIAL_VISIBLE_MESSAGES: usize = 24;
const LABEL_WIDTH: usize = 9;

pub async fn run(unix_socket: PathBuf) -> Result<()> {
    let mut tui = TuiApp::new(unix_socket).await?;
    tui.run().await
}

struct TuiApp {
    unix_socket: PathBuf,
    session_id: String,
    session: SessionGetResponse,
    rendered_messages: usize,
    input: InputBuffer,
}

struct SubmitRenderState {
    streamed_assistant: bool,
    skipped_user: bool,
    skipped_assistant: bool,
}

impl Default for SubmitRenderState {
    fn default() -> Self {
        Self {
            streamed_assistant: false,
            skipped_user: true,
            skipped_assistant: false,
        }
    }
}

#[derive(Default)]
struct StreamRenderState {
    streamed_assistant: bool,
    assistant_started: bool,
    assistant_ended_with_newline: bool,
}

impl TuiApp {
    async fn new(unix_socket: PathBuf) -> Result<Self> {
        let sessions: SessionListResponse =
            request(unix_socket.clone(), ApiMethod::SessionList, json!({})).await?;
        let session_id = resolve_initial_session_id(&sessions);
        let session = fetch_session(unix_socket.clone(), session_id.clone()).await?;

        Ok(Self {
            unix_socket,
            session_id,
            session,
            rendered_messages: 0,
            input: InputBuffer::default(),
        })
    }

    async fn run(&mut self) -> Result<()> {
        self.print_banner();
        self.render_initial_messages();

        loop {
            self.refresh_session().await?;
            self.render_new_messages(SubmitRenderState::default());

            let Some(input) = self.read_prompt_line("you> ")? else {
                println!();
                break;
            };
            let trimmed = input.trim();
            if trimmed.is_empty() {
                continue;
            }
            if matches!(trimmed, "/exit" | "/quit") {
                break;
            }

            self.submit_input(trimmed.to_string()).await?;
        }

        Ok(())
    }

    fn read_prompt_line(&mut self, prompt: &str) -> Result<Option<String>> {
        let _guard = RawModeGuard::enter()?;
        self.input.render(prompt)?;

        loop {
            match event::read().context("failed to read terminal event")? {
                Event::Key(key) => {
                    if let Some(result) = self.input.handle_key(prompt, key)? {
                        return Ok(result);
                    }
                }
                Event::Resize(_, _) => self.input.render(prompt)?,
                _ => {}
            }
        }
    }

    async fn refresh_session(&mut self) -> Result<()> {
        self.session = fetch_session(self.unix_socket.clone(), self.session_id.clone()).await?;
        Ok(())
    }

    async fn submit_input(&mut self, message: String) -> Result<()> {
        let previous_message_count = self.session.messages.len();
        let actor = ActorIdentity {
            kind: "tui".into(),
            id: "operator.local".into(),
            label: "agentjax-tui".into(),
        };
        let request = RequestEnvelope {
            id: RequestId(format!("req_{}", chrono::Utc::now().timestamp_millis())),
            method: ApiMethod::SessionSend,
            params: json!({
                "session_id": self.session_id,
                "message": SessionMessage::user(message),
                "stream": true,
            }),
            meta: Some(RequestMeta {
                requester: Some(actor.clone()),
                session_id: Some(self.session_id.clone()),
                surface_id: Some("tui.local".into()),
                ..RequestMeta::default()
            }),
        };
        let client = UnixSocketClient::new(self.unix_socket.clone(), actor);
        let mut stream_state = StreamRenderState::default();

        let response = client
            .request_with_streams(request, |stream| {
                if handle_stream(stream, &mut stream_state).is_err() {
                    eprintln!(
                        "{}",
                        "warning: failed to render stream chunk"
                            .with(Color::Yellow)
                            .attribute(Attribute::Bold)
                    );
                }
            })
            .await;

        if stream_state.assistant_started && !stream_state.assistant_ended_with_newline {
            println!();
        }

        match response {
            Ok(response) if response.ok => {
                let Some(result) = response.result else {
                    print_note("send failed: missing response result", Color::Red);
                    return Ok(());
                };

                let send_response: SessionSendResponse = serde_json::from_value(result)
                    .context("failed to decode session.send response")?;
                self.refresh_session().await?;
                self.render_new_messages_from(
                    previous_message_count,
                    SubmitRenderState {
                        streamed_assistant: stream_state.streamed_assistant,
                        skipped_user: true,
                        skipped_assistant: false,
                    },
                );

                if !stream_state.streamed_assistant && send_response.stream_id.is_some() {
                    print_note(
                        "reply completed without visible stream chunks",
                        Color::DarkGrey,
                    );
                }
            }
            Ok(response) => {
                let message = response
                    .error
                    .map(|error| error.message)
                    .unwrap_or_else(|| "request failed".into());
                print_note(format!("send failed: {message}"), Color::Red);
            }
            Err(error) => {
                print_note(format!("send failed: {error}"), Color::Red);
            }
        }

        Ok(())
    }

    fn print_banner(&self) {
        println!(
            "{}",
            "AgentJax".with(Color::Cyan).attribute(Attribute::Bold)
        );
        if let Some(title) = self
            .session
            .session
            .title
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            println!("{}", title.with(Color::White).attribute(Attribute::Bold));
        }
        println!(
            "{} {}",
            "session".with(Color::DarkGrey),
            self.session_id.as_str().with(Color::DarkGrey)
        );
        println!("{}", "type /exit to quit".with(Color::DarkGrey));
        println!();
    }

    fn render_initial_messages(&mut self) {
        let renderable = collect_renderable_messages(&self.session);
        if renderable.is_empty() {
            print_note("no messages yet", Color::DarkGrey);
            self.rendered_messages = self.session.messages.len();
            return;
        }

        let hidden = renderable.len().saturating_sub(INITIAL_VISIBLE_MESSAGES);
        if hidden > 0 {
            print_note(
                format!("showing latest {INITIAL_VISIBLE_MESSAGES} messages"),
                Color::DarkGrey,
            );
        }

        for message in renderable.into_iter().skip(hidden) {
            print_message(message);
        }
        self.rendered_messages = self.session.messages.len();
    }

    fn render_new_messages(&mut self, state: SubmitRenderState) {
        self.render_new_messages_from(self.rendered_messages, state);
    }

    fn render_new_messages_from(&mut self, start: usize, mut state: SubmitRenderState) {
        for message in self.session.messages.iter().skip(start) {
            match message.normalized_kind() {
                SessionMessageKind::User => {
                    if state.skipped_user {
                        print_message(message);
                    } else {
                        state.skipped_user = true;
                    }
                }
                SessionMessageKind::Assistant => {
                    if state.streamed_assistant && !state.skipped_assistant {
                        state.skipped_assistant = true;
                    } else {
                        print_message(message);
                        state.skipped_assistant = true;
                    }
                }
                SessionMessageKind::Runtime => print_message(message),
                SessionMessageKind::ToolResult | SessionMessageKind::System => {}
            }
        }

        self.rendered_messages = self.session.messages.len();
    }
}

fn resolve_initial_session_id(sessions: &SessionListResponse) -> String {
    sessions
        .items
        .first()
        .map(|item| item.session_id.clone())
        .unwrap_or_else(|| "session.default".into())
}

fn collect_renderable_messages(session: &SessionGetResponse) -> Vec<&SessionMessage> {
    session
        .messages
        .iter()
        .filter(|message| {
            matches!(
                message.normalized_kind(),
                SessionMessageKind::User
                    | SessionMessageKind::Assistant
                    | SessionMessageKind::Runtime
            )
        })
        .collect()
}

fn print_message(message: &SessionMessage) {
    let (label, color) = match message.normalized_kind() {
        SessionMessageKind::User => ("you", Color::Blue),
        SessionMessageKind::Assistant => ("agent", Color::Green),
        SessionMessageKind::Runtime => ("note", Color::DarkGrey),
        SessionMessageKind::ToolResult => ("tool", Color::Yellow),
        SessionMessageKind::System => ("system", Color::Magenta),
    };
    print_labeled_text(label, color, &message.content);
}

fn print_note(message: impl AsRef<str>, color: Color) {
    print_labeled_text("note", color, message.as_ref());
}

fn print_labeled_text(label: &str, color: Color, content: &str) {
    let formatted_label = format!("{label:>LABEL_WIDTH$}");
    let mut lines = content.lines();

    if let Some(first) = lines.next() {
        println!(
            "{} {}",
            formatted_label.with(color).attribute(Attribute::Bold),
            first
        );
        for line in lines {
            println!("{:>LABEL_WIDTH$} {}", "", line);
        }
    } else {
        println!("{}", formatted_label.with(color).attribute(Attribute::Bold));
    }

    println!();
}

#[derive(Default)]
struct InputBuffer {
    text: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    history_draft: String,
}

impl InputBuffer {
    fn handle_key(&mut self, prompt: &str, key: KeyEvent) -> Result<Option<Option<String>>> {
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.clear_prompt_line()?;
                return Ok(Some(None));
            }
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                if self.text.is_empty() {
                    self.clear_prompt_line()?;
                    return Ok(Some(None));
                }
            }
            KeyCode::Esc => {
                self.clear_prompt_line()?;
                return Ok(Some(None));
            }
            KeyCode::Enter => {
                let submitted = self.text.clone();
                self.commit_history();
                execute!(io::stdout(), MoveToColumn(0), Clear(ClearType::CurrentLine))?;
                print!("{prompt}{submitted}");
                println!();
                io::stdout().flush()?;
                self.text.clear();
                self.cursor = 0;
                self.history_index = None;
                self.history_draft.clear();
                return Ok(Some(Some(submitted)));
            }
            KeyCode::Backspace => self.backspace(),
            KeyCode::Left => self.move_left(),
            KeyCode::Right => self.move_right(),
            KeyCode::Home => self.move_home(),
            KeyCode::End => self.move_end(),
            KeyCode::Up => self.history_up(),
            KeyCode::Down => self.history_down(),
            KeyCode::Char(ch)
                if !key.modifiers.contains(KeyModifiers::CONTROL)
                    && !key.modifiers.contains(KeyModifiers::ALT) =>
            {
                self.insert_char(ch);
            }
            _ => {}
        }

        self.render(prompt)?;
        Ok(None)
    }

    fn insert_char(&mut self, ch: char) {
        let mut chars = self.chars();
        chars.insert(self.cursor, ch);
        self.text = chars.into_iter().collect();
        self.cursor += 1;
        self.reset_history_navigation();
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut chars = self.chars();
        chars.remove(self.cursor - 1);
        self.text = chars.into_iter().collect();
        self.cursor -= 1;
        self.reset_history_navigation();
    }

    fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.char_len());
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.char_len();
    }

    fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        match self.history_index {
            None => {
                self.history_draft = self.text.clone();
                self.history_index = Some(self.history.len().saturating_sub(1));
            }
            Some(0) => {}
            Some(index) => self.history_index = Some(index - 1),
        }
        if let Some(index) = self.history_index {
            self.set_text(self.history[index].clone());
        }
    }

    fn history_down(&mut self) {
        let Some(index) = self.history_index else {
            return;
        };
        if index + 1 >= self.history.len() {
            self.history_index = None;
            self.set_text(self.history_draft.clone());
            self.history_draft.clear();
            return;
        }
        self.history_index = Some(index + 1);
        self.set_text(self.history[index + 1].clone());
    }

    fn commit_history(&mut self) {
        let trimmed = self.text.trim();
        if trimmed.is_empty() {
            return;
        }
        if self.history.last().map(|item| item.as_str()) != Some(self.text.as_str()) {
            self.history.push(self.text.clone());
        }
    }

    fn reset_history_navigation(&mut self) {
        if self.history_index.is_some() {
            self.history_index = None;
            self.history_draft.clear();
        }
    }

    fn set_text(&mut self, text: String) {
        self.text = text;
        self.cursor = self.char_len();
    }

    fn render(&self, prompt: &str) -> Result<()> {
        let prompt_width = UnicodeWidthStr::width(prompt);
        let cursor_width = prompt_width + display_width(&self.text_before_cursor());

        execute!(
            io::stdout(),
            MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            cursor::Show
        )?;
        print!("{prompt}{}", self.text);
        execute!(
            io::stdout(),
            MoveToColumn((cursor_width as u16).saturating_add(1))
        )?;
        io::stdout().flush()?;
        Ok(())
    }

    fn clear_prompt_line(&self) -> Result<()> {
        execute!(
            io::stdout(),
            MoveToColumn(0),
            Clear(ClearType::CurrentLine),
            cursor::Show
        )?;
        io::stdout().flush()?;
        Ok(())
    }

    fn chars(&self) -> Vec<char> {
        self.text.chars().collect()
    }

    fn char_len(&self) -> usize {
        self.text.chars().count()
    }

    fn text_before_cursor(&self) -> String {
        self.text.chars().take(self.cursor).collect()
    }
}

struct RawModeGuard;

impl RawModeGuard {
    fn enter() -> Result<Self> {
        terminal::enable_raw_mode().context("failed to enable raw mode")?;
        execute!(io::stdout(), cursor::Show).context("failed to show cursor")?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), cursor::Show);
    }
}

fn display_width(text: &str) -> usize {
    text.chars()
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum()
}

fn handle_stream(stream: &StreamEnvelope, state: &mut StreamRenderState) -> io::Result<()> {
    match stream.phase {
        StreamPhase::Start => {
            if stream.event == "turn.started" {
                print_note("thinking", Color::DarkGrey);
            }
        }
        StreamPhase::Chunk => match stream.event.as_str() {
            "assistant.plan" => {
                finish_active_assistant_line(state);
                let message = stream
                    .data
                    .get("message")
                    .and_then(|value| value.as_str())
                    .unwrap_or("I am checking the context and preparing the next steps.");
                let label = format!("{:>LABEL_WIDTH$}", "agent")
                    .with(Color::Green)
                    .attribute(Attribute::Bold);
                println!("{label} {message}");
            }
            "assistant.text.delta" => {
                let Some(text) = stream.data.get("text").and_then(|value| value.as_str()) else {
                    return Ok(());
                };
                if !state.assistant_started {
                    let label = format!("{:>LABEL_WIDTH$}", "agent")
                        .with(Color::Green)
                        .attribute(Attribute::Bold);
                    print!("{label} ");
                    state.assistant_started = true;
                }
                print!("{text}");
                io::stdout().flush()?;
                state.streamed_assistant = true;
                state.assistant_ended_with_newline = text.ends_with('\n');
            }
            "tool_call.started" => {
                finish_active_assistant_line(state);
                let tool_name = stream
                    .data
                    .get("tool_name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("tool");
                print_note(format!("using {tool_name}"), Color::Yellow);
            }
            "tool_call.completed" => {
                finish_active_assistant_line(state);
                let tool_name = stream
                    .data
                    .get("tool_name")
                    .and_then(|value| value.as_str())
                    .unwrap_or("tool");
                print_note(format!("{tool_name} completed"), Color::DarkGrey);
            }
            "runtime.waiting" => {
                finish_active_assistant_line(state);
                let message = stream
                    .data
                    .get("message")
                    .and_then(|value| value.as_str())
                    .unwrap_or("task is waiting");
                print_note(message, Color::DarkGrey);
            }
            "assistant.completed" | "turn.completed" => {
                finish_active_assistant_line(state);
            }
            _ => {}
        },
        StreamPhase::End => {
            finish_active_assistant_line(state);
        }
        StreamPhase::Error => {
            finish_active_assistant_line(state);
            let message = stream
                .data
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("stream error");
            print_note(format!("stream error: {message}"), Color::Red);
        }
    }

    Ok(())
}

fn finish_active_assistant_line(state: &mut StreamRenderState) {
    if state.assistant_started && !state.assistant_ended_with_newline {
        println!();
    }
    state.assistant_started = false;
    state.assistant_ended_with_newline = false;
}

async fn fetch_session(unix_socket: PathBuf, session_id: String) -> Result<SessionGetResponse> {
    request(
        unix_socket,
        ApiMethod::SessionGet,
        json!({ "session_id": session_id }),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{
        collect_renderable_messages, display_width, resolve_initial_session_id, InputBuffer,
        INITIAL_VISIBLE_MESSAGES,
    };
    use crate::{
        api::{SessionGetResponse, SessionListItem, SessionListResponse, SessionMessage},
        domain::{ObjectMeta, Session, SessionMode, SessionStatus},
    };

    #[test]
    fn resolves_first_session_id() {
        let sessions = SessionListResponse {
            items: vec![SessionListItem {
                session_id: "session.alpha".into(),
                agent_id: "agent.default".into(),
                title: Some("Alpha".into()),
                status: SessionStatus::Active,
                channel_id: None,
                surface_id: None,
                last_activity_at: None,
            }],
        };

        assert_eq!(resolve_initial_session_id(&sessions), "session.alpha");
    }

    #[test]
    fn collect_renderable_messages_hides_tool_results() {
        let session = sample_session(vec![
            SessionMessage::user("hello"),
            SessionMessage::tool_result("{\"ok\":true}"),
            SessionMessage::runtime("sleeping for 10s"),
            SessionMessage::assistant("done"),
        ]);

        let renderable = collect_renderable_messages(&session);
        assert_eq!(renderable.len(), 3);
        assert!(renderable.len() <= INITIAL_VISIBLE_MESSAGES);
    }

    #[test]
    fn input_buffer_backspace_handles_multibyte_characters() {
        let mut input = InputBuffer {
            text: "你好a".into(),
            cursor: 3,
            ..InputBuffer::default()
        };

        input.backspace();
        assert_eq!(input.text, "你好");
        assert_eq!(input.cursor, 2);

        input.backspace();
        assert_eq!(input.text, "你");
        assert_eq!(input.cursor, 1);
    }

    #[test]
    fn input_buffer_history_navigation_round_trips_draft() {
        let mut input = InputBuffer {
            history: vec!["first".into(), "second".into()],
            text: "draft".into(),
            cursor: 5,
            ..InputBuffer::default()
        };

        input.history_up();
        assert_eq!(input.text, "second");
        input.history_up();
        assert_eq!(input.text, "first");
        input.history_down();
        assert_eq!(input.text, "second");
        input.history_down();
        assert_eq!(input.text, "draft");
    }

    #[test]
    fn display_width_counts_wide_characters() {
        assert_eq!(display_width("abc"), 3);
        assert_eq!(display_width("你好"), 4);
    }

    fn sample_session(messages: Vec<SessionMessage>) -> SessionGetResponse {
        SessionGetResponse {
            session: Session {
                meta: ObjectMeta::new("session.default", "2026-04-12"),
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
            messages,
            events: Vec::new(),
        }
    }
}
