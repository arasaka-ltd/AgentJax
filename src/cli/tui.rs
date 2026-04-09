use std::{io, path::PathBuf, time::Duration};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    DefaultTerminal,
};
use serde_json::json;

use crate::{
    api::{ApiMethod, SessionGetResponse, SessionListResponse, SessionSendResponse},
    cli::{request, session_send},
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
    session_id: String,
    sessions: SessionListResponse,
    session: SessionGetResponse,
    input: String,
    status: String,
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
        let session: SessionGetResponse = request(
            unix_socket.clone(),
            ApiMethod::SessionGet,
            json!({ "session_id": session_id }),
        )
        .await?;

        Ok(Self {
            unix_socket,
            session_id,
            sessions,
            session,
            input: String::new(),
            status: String::from("Ready"),
        })
    }

    async fn run(&mut self, mut terminal: DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|frame| self.draw(frame))?;

            if !event::poll(Duration::from_millis(100))? {
                continue;
            }

            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Esc => break,
                    KeyCode::Char('c') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => break,
                    KeyCode::Enter => {
                        let message = self.input.trim().to_string();
                        if message.is_empty() {
                            continue;
                        }

                        self.status = String::from("Sending...");
                        let send_result: Result<SessionSendResponse> =
                            session_send(self.unix_socket.clone(), self.session_id.clone(), message, false)
                                .await;
                        match send_result {
                            Ok(_) => {
                                self.session = request(
                                    self.unix_socket.clone(),
                                    ApiMethod::SessionGet,
                                    json!({ "session_id": self.session_id }),
                                )
                                .await?;
                                self.status = String::from("Sent");
                            }
                            Err(error) => {
                                self.session = request(
                                    self.unix_socket.clone(),
                                    ApiMethod::SessionGet,
                                    json!({ "session_id": self.session_id }),
                                )
                                .await?;
                                self.status = format!("Error: {error}");
                            }
                        }
                        self.input.clear();
                    }
                    KeyCode::Backspace => {
                        self.input.pop();
                    }
                    KeyCode::Char(ch) => {
                        self.input.push(ch);
                    }
                    _ => {}
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
                Constraint::Min(10),
                Constraint::Length(3),
                Constraint::Length(2),
            ])
            .split(frame.area());

        let top = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(28), Constraint::Min(20)])
            .split(layout[1]);

        let session_items: Vec<ListItem> = self
            .sessions
            .items
            .iter()
            .map(|item| {
                let label = if item.session_id == self.session_id {
                    format!("> {}", item.session_id)
                } else {
                    item.session_id.clone()
                };
                ListItem::new(Line::from(label))
            })
            .collect();

        let messages: Vec<Line> = if self.session.messages.is_empty() {
            vec![Line::from("(no messages yet)")]
        } else {
            self.session
                .messages
                .iter()
                .flat_map(|message| {
                    vec![
                        Line::from(vec![
                            Span::styled(
                                format!("{}: ", message.role),
                                Style::default().add_modifier(Modifier::BOLD),
                            ),
                            Span::raw(&message.content),
                        ]),
                        Line::from(""),
                    ]
                })
                .collect()
        };

        let header = Paragraph::new(format!(
            "AgentJax TUI  |  session: {}  |  status: {:?}",
            self.session.session.session_id, self.session.session.status
        ))
        .block(Block::default().borders(Borders::ALL).title("Header"));

        let sessions = List::new(session_items)
            .block(Block::default().borders(Borders::ALL).title("Sessions"));

        let chat = Paragraph::new(messages)
            .block(Block::default().borders(Borders::ALL).title("Chat"))
            .wrap(Wrap { trim: false });

        let input = Paragraph::new(self.input.as_str())
            .block(Block::default().borders(Borders::ALL).title("Input"));

        let status = Paragraph::new(self.status.as_str())
            .block(Block::default().borders(Borders::ALL).title("Status"));

        frame.render_widget(header, layout[0]);
        frame.render_widget(sessions, top[0]);
        frame.render_widget(chat, top[1]);
        frame.render_widget(input, layout[2]);
        frame.render_widget(status, layout[3]);
    }
}
