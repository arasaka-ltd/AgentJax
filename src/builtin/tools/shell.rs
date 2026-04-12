use std::{
    collections::{HashMap, VecDeque},
    env,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, OnceLock,
    },
};

use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Map, Value};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::Mutex as AsyncMutex,
    time::{timeout, Duration},
};

use crate::{
    builtin::tools::{support, ToolDescriptor, ToolPlugin},
    core::Plugin,
    domain::{Permission, PluginCapability, PluginManifest, ToolCall, ToolCapability},
};

const DEFAULT_SHELL: &str = "/bin/sh";
const DEFAULT_CAPTURE_LIMIT: usize = 12_000;
const DEFAULT_SESSION_READ_LIMIT: usize = 8_000;
const SESSION_BUFFER_LIMIT: usize = 128_000;
const DEFAULT_COLUMNS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;
const EXEC_START_MARKER: &str = "__AJX_EXEC_START__";
const EXEC_END_MARKER: &str = "__AJX_EXEC_END__";

static SHELL_MANAGER: OnceLock<Arc<ShellSessionManager>> = OnceLock::new();
static SHELL_SESSION_SEQ: AtomicU64 = AtomicU64::new(1);
static SHELL_EXEC_SEQ: AtomicU64 = AtomicU64::new(1);

fn manager() -> &'static Arc<ShellSessionManager> {
    SHELL_MANAGER.get_or_init(|| Arc::new(ShellSessionManager::default()))
}

#[derive(Debug, Clone, Serialize)]
struct ShellOutputChunk {
    seq: u64,
    stream: &'static str,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellSessionStatus {
    Idle,
    Running,
    Closed,
    Failed,
}

impl ShellSessionStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Closed => "closed",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ShellExecutionMode {
    Stateless,
    SessionBound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
enum ShellExecutionStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
    TimedOut,
}

#[derive(Debug, Clone, Serialize)]
struct ShellExecutionRecord {
    exec_id: String,
    session_id: Option<String>,
    mode: ShellExecutionMode,
    command: String,
    status: ShellExecutionStatus,
    exit_code: Option<i32>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    timed_out: bool,
    interrupted: bool,
    detached: bool,
}

#[derive(Debug)]
struct ShellSessionState {
    session_id: String,
    shell: String,
    cwd: String,
    title: Option<String>,
    pty: bool,
    status: ShellSessionStatus,
    active_exec_id: Option<String>,
    last_exit_code: Option<i32>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    columns: u16,
    rows: u16,
    next_seq: u64,
    chunks: VecDeque<ShellOutputChunk>,
    chunk_bytes: usize,
    active_execution: Option<ShellExecutionRecord>,
    last_execution: Option<ShellExecutionRecord>,
    pending_completed_execution: Option<ShellExecutionRecord>,
}

impl ShellSessionState {
    fn new(
        session_id: String,
        shell: String,
        cwd: String,
        title: Option<String>,
        pty: bool,
    ) -> Self {
        let now = Utc::now();
        Self {
            session_id,
            shell,
            cwd,
            title,
            pty,
            status: ShellSessionStatus::Idle,
            active_exec_id: None,
            last_exit_code: None,
            created_at: now,
            updated_at: now,
            columns: DEFAULT_COLUMNS,
            rows: DEFAULT_ROWS,
            next_seq: 1,
            chunks: VecDeque::new(),
            chunk_bytes: 0,
            active_execution: None,
            last_execution: None,
            pending_completed_execution: None,
        }
    }

    fn append_chunk(&mut self, stream: &'static str, text: String) {
        if text.is_empty() {
            return;
        }
        let bytes = text.len();
        let chunk = ShellOutputChunk {
            seq: self.next_seq,
            stream,
            text,
        };
        self.next_seq += 1;
        self.updated_at = Utc::now();
        self.chunk_bytes += bytes;
        self.chunks.push_back(chunk);
        while self.chunk_bytes > SESSION_BUFFER_LIMIT {
            if let Some(removed) = self.chunks.pop_front() {
                self.chunk_bytes = self.chunk_bytes.saturating_sub(removed.text.len());
            } else {
                break;
            }
        }
    }

    fn finish_exec(&mut self, exec_id: &str, exit_code: i32, cwd: String) {
        if self.active_exec_id.as_deref() == Some(exec_id) {
            self.active_exec_id = None;
        }
        self.last_exit_code = Some(exit_code);
        self.cwd = cwd;
        let mut completed = self
            .active_execution
            .take()
            .unwrap_or(ShellExecutionRecord {
                exec_id: exec_id.to_string(),
                session_id: Some(self.session_id.clone()),
                mode: ShellExecutionMode::SessionBound,
                command: String::new(),
                status: ShellExecutionStatus::Completed,
                exit_code: Some(exit_code),
                started_at: Utc::now(),
                finished_at: None,
                timed_out: false,
                interrupted: false,
                detached: false,
            });
        completed.exit_code = Some(exit_code);
        completed.finished_at = Some(Utc::now());
        completed.status = if completed.interrupted {
            ShellExecutionStatus::Interrupted
        } else if completed.timed_out {
            ShellExecutionStatus::TimedOut
        } else if exit_code == 0 {
            ShellExecutionStatus::Completed
        } else {
            ShellExecutionStatus::Failed
        };
        self.pending_completed_execution = Some(completed.clone());
        self.last_execution = Some(completed);
        self.status = ShellSessionStatus::Idle;
        self.updated_at = Utc::now();
    }
}

struct ShellSessionHandle {
    state: Arc<Mutex<ShellSessionState>>,
    child: Arc<AsyncMutex<Child>>,
    process_group_id: i32,
    pty: bool,
}

type SessionStateRef = Arc<Mutex<ShellSessionState>>;
type SessionChildRef = Arc<AsyncMutex<Child>>;

#[derive(Default)]
struct ShellSessionManager {
    sessions: Mutex<HashMap<String, ShellSessionHandle>>,
}

impl ShellSessionManager {
    async fn stateless_exec(
        &self,
        command: &str,
        cwd: Option<&str>,
        env_map: &Map<String, Value>,
        shell: Option<&str>,
        timeout_secs: Option<u64>,
        capture_limit: Option<usize>,
    ) -> Result<Value> {
        let resolved_shell = resolve_shell(shell);
        let resolved_cwd = resolve_cwd(cwd)?;
        let limit = capture_limit.unwrap_or(DEFAULT_CAPTURE_LIMIT);

        let mut cmd = Command::new(&resolved_shell);
        cmd.arg("-lc")
            .arg(command)
            .current_dir(&resolved_cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        cmd.as_std_mut().process_group(0);
        apply_env(&mut cmd, env_map)?;

        let child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn shell {}", resolved_shell))?;
        let process_group_id = child.id().unwrap_or_default() as i32;
        let timeout_budget = Duration::from_secs(timeout_secs.unwrap_or(120));
        let mut child = child;
        let stdout_handle = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("shell command stdout is unavailable"))?;
        let stderr_handle = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("shell command stderr is unavailable"))?;
        let stdout_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stdout_handle);
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;
            Ok::<Vec<u8>, std::io::Error>(bytes)
        });
        let stderr_task = tokio::spawn(async move {
            let mut reader = BufReader::new(stderr_handle);
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).await?;
            Ok::<Vec<u8>, std::io::Error>(bytes)
        });

        let (status, exit_code, timed_out) = match timeout(timeout_budget, child.wait()).await {
            Ok(wait) => {
                let status = wait.context("failed to wait for shell command")?;
                ("completed", status.code().unwrap_or_default(), false)
            }
            Err(_) => {
                if process_group_id > 0 {
                    let _ = signal_process_group(process_group_id, libc::SIGKILL);
                }
                let status = child
                    .wait()
                    .await
                    .context("failed to wait for timed out shell command")?;
                ("timed_out", status.code().unwrap_or(-1), true)
            }
        };
        let stdout = String::from_utf8_lossy(
            &stdout_task
                .await
                .context("failed to join stdout reader task")?
                .context("failed to read shell stdout")?,
        )
        .to_string();
        let stderr = String::from_utf8_lossy(
            &stderr_task
                .await
                .context("failed to join stderr reader task")?
                .context("failed to read shell stderr")?,
        )
        .to_string();

        let (stdout, stdout_truncated) = clip_text_bytes(&stdout, limit);
        let (stderr, stderr_truncated) =
            clip_text_bytes(&stderr, limit.saturating_sub(stdout.len()));
        let combined_source = if stderr.is_empty() {
            stdout.clone()
        } else if stdout.is_empty() {
            stderr.clone()
        } else {
            format!("{stdout}{stderr}")
        };
        let (combined_output, combined_truncated) = clip_text_bytes(&combined_source, limit);

        Ok(json!({
            "exit_code": exit_code,
            "status": status,
            "stdout": stdout,
            "stderr": stderr,
            "combined_output": combined_output,
            "cwd": resolved_cwd.display().to_string(),
            "timed_out": timed_out,
            "truncated": stdout_truncated || stderr_truncated || combined_truncated,
            "execution": {
                "exec_id": format!("shexec_{}", SHELL_EXEC_SEQ.fetch_add(1, Ordering::Relaxed)),
                "session_id": Value::Null,
                "mode": ShellExecutionMode::Stateless,
                "command": command,
                "status": if timed_out {
                    ShellExecutionStatus::TimedOut
                } else if exit_code == 0 {
                    ShellExecutionStatus::Completed
                } else {
                    ShellExecutionStatus::Failed
                },
                "exit_code": exit_code,
                "started_at": Utc::now(),
                "finished_at": Utc::now(),
                "timed_out": timed_out,
                "interrupted": false,
                "detached": false
            }
        }))
    }

    async fn open_session(
        &self,
        cwd: Option<&str>,
        env_map: &Map<String, Value>,
        shell: Option<&str>,
        pty: bool,
        title: Option<String>,
    ) -> Result<Value> {
        let resolved_shell = resolve_shell(shell);
        let resolved_cwd = resolve_cwd(cwd)?;
        let columns = DEFAULT_COLUMNS;
        let rows = DEFAULT_ROWS;
        let session_id = format!(
            "shsess_{}",
            SHELL_SESSION_SEQ.fetch_add(1, Ordering::Relaxed)
        );

        let mut cmd = if pty {
            let mut script = Command::new("script");
            script
                .arg("-q")
                .arg("/dev/null")
                .arg(&resolved_shell)
                .arg("-i");
            script
        } else {
            let mut shell_cmd = Command::new(&resolved_shell);
            shell_cmd.arg("-i");
            shell_cmd
        };
        cmd.current_dir(&resolved_cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        cmd.as_std_mut().process_group(0);
        apply_env(&mut cmd, env_map)?;

        let mut child = cmd
            .spawn()
            .with_context(|| format!("failed to spawn session shell {}", resolved_shell))?;
        let process_group_id = child.id().unwrap_or_default() as i32;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("session shell stdout is unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("session shell stderr is unavailable"))?;

        let state = Arc::new(Mutex::new(ShellSessionState::new(
            session_id.clone(),
            resolved_shell.clone(),
            resolved_cwd.display().to_string(),
            title.clone(),
            pty,
        )));
        {
            let mut locked = state.lock().expect("shell session state lock poisoned");
            locked.columns = columns;
            locked.rows = rows;
        }
        let child = Arc::new(AsyncMutex::new(child));

        spawn_output_pump(stdout, Arc::clone(&state), "stdout");
        spawn_output_pump(stderr, Arc::clone(&state), "stderr");

        self.sessions
            .lock()
            .expect("shell sessions lock poisoned")
            .insert(
                session_id.clone(),
                ShellSessionHandle {
                    state: Arc::clone(&state),
                    child,
                    process_group_id,
                    pty,
                },
            );

        if pty {
            self.apply_resize_command(&session_id, columns, rows)
                .await?;
        }

        Ok(json!({
            "session_id": session_id,
            "status": "idle",
            "cwd": resolved_cwd.display().to_string(),
            "shell": resolved_shell,
            "pty": pty,
            "title": title,
            "cols": columns,
            "rows": rows,
        }))
    }

    async fn session_exec(
        &self,
        session_id: &str,
        command: &str,
        timeout_secs: Option<u64>,
        detach: bool,
    ) -> Result<Value> {
        let (state, child) = self.session_refs(session_id)?;
        self.refresh_session_process(&state, &child).await?;

        let exec_id = format!("shexec_{}", SHELL_EXEC_SEQ.fetch_add(1, Ordering::Relaxed));
        {
            let mut locked = state.lock().expect("shell session state lock poisoned");
            if locked.status == ShellSessionStatus::Closed {
                bail!("shell session is closed: {session_id}");
            }
            if locked.active_exec_id.is_some() {
                bail!("shell session already has a running foreground command: {session_id}");
            }
            locked.status = ShellSessionStatus::Running;
            locked.active_exec_id = Some(exec_id.clone());
            locked.updated_at = Utc::now();
            locked.active_execution = Some(ShellExecutionRecord {
                exec_id: exec_id.clone(),
                session_id: Some(session_id.to_string()),
                mode: ShellExecutionMode::SessionBound,
                command: command.to_string(),
                status: ShellExecutionStatus::Running,
                exit_code: None,
                started_at: Utc::now(),
                finished_at: None,
                timed_out: false,
                interrupted: false,
                detached: detach,
            });
        }

        let timeout_budget = timeout_secs.unwrap_or(300);
        let script = build_session_exec_script(&exec_id, command, timeout_budget, detach);
        let mut process = child.lock().await;
        let stdin = process
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("shell session stdin is unavailable"))?;
        if let Err(error) = stdin.write_all(script.as_bytes()).await {
            let mut locked = state.lock().expect("shell session state lock poisoned");
            locked.status = ShellSessionStatus::Failed;
            locked.active_exec_id = None;
            return Err(anyhow!("failed to send command to shell session: {error}"));
        }
        stdin.flush().await?;

        Ok(json!({
            "session_id": session_id,
            "exec_id": exec_id,
            "accepted": true,
            "status": "running",
        }))
    }

    async fn session_read(
        &self,
        session_id: &str,
        since_seq: u64,
        max_bytes: usize,
    ) -> Result<Value> {
        let (state, child) = self.session_refs(session_id)?;
        self.refresh_session_process(&state, &child).await?;

        let mut locked = state.lock().expect("shell session state lock poisoned");
        let mut emitted = Vec::new();
        let mut used = 0;
        let mut truncated = false;
        for chunk in locked.chunks.iter().filter(|chunk| chunk.seq > since_seq) {
            if used >= max_bytes {
                truncated = true;
                break;
            }
            let remaining = max_bytes.saturating_sub(used);
            let (text, chunk_truncated) = clip_text_bytes(&chunk.text, remaining);
            if text.is_empty() && remaining == 0 {
                truncated = true;
                break;
            }
            used += text.len();
            emitted.push(json!({
                "seq": chunk.seq,
                "stream": chunk.stream,
                "text": text,
            }));
            if chunk_truncated {
                truncated = true;
                break;
            }
        }
        let completed_exec = locked.pending_completed_execution.take();

        Ok(json!({
            "session_id": locked.session_id,
            "status": locked.status.as_str(),
            "active_exec_id": locked.active_exec_id,
            "cwd": locked.cwd,
            "shell": locked.shell,
            "pty": locked.pty,
            "title": locked.title,
            "cols": locked.columns,
            "rows": locked.rows,
            "seq": locked.next_seq.saturating_sub(1),
            "chunks": emitted,
            "last_exit_code": locked.last_exit_code,
            "active_exec": locked.active_execution,
            "last_exec": locked.last_execution,
            "completed_exec": completed_exec,
            "truncated": truncated,
        }))
    }

    async fn list_sessions(&self) -> Result<Value> {
        let refs = {
            let locked = self.sessions.lock().expect("shell sessions lock poisoned");
            locked
                .values()
                .map(|handle| (Arc::clone(&handle.state), Arc::clone(&handle.child)))
                .collect::<Vec<_>>()
        };

        let mut sessions = Vec::new();
        for (state, child) in refs {
            self.refresh_session_process(&state, &child).await?;
            let locked = state.lock().expect("shell session state lock poisoned");
            sessions.push(json!({
                "session_id": locked.session_id,
                "title": locked.title,
                "status": locked.status.as_str(),
                "cwd": locked.cwd,
                "shell": locked.shell,
                "pty": locked.pty,
                "cols": locked.columns,
                "rows": locked.rows,
                "active_exec_id": locked.active_exec_id,
                "active_exec": locked.active_execution,
                "last_exec": locked.last_execution,
                "created_at": locked.created_at,
                "last_active_at": locked.updated_at,
            }));
        }

        Ok(json!({ "sessions": sessions }))
    }

    async fn close_session(&self, session_id: &str, force: bool) -> Result<Value> {
        let handle = {
            let mut locked = self.sessions.lock().expect("shell sessions lock poisoned");
            locked
                .remove(session_id)
                .ok_or_else(|| anyhow!("shell session not found: {session_id}"))?
        };

        let running = {
            let state = handle
                .state
                .lock()
                .expect("shell session state lock poisoned");
            state.status == ShellSessionStatus::Running
        };
        if running && !force {
            self.sessions
                .lock()
                .expect("shell sessions lock poisoned")
                .insert(session_id.to_string(), handle);
            bail!("shell session has a running command; set force=true to close it");
        }

        let mut child = handle.child.lock().await;
        if handle.process_group_id > 0 {
            let _ = signal_process_group(handle.process_group_id, libc::SIGTERM);
        }
        let _ = child.start_kill();
        let _ = child.wait().await;
        drop(child);

        let mut state = handle
            .state
            .lock()
            .expect("shell session state lock poisoned");
        state.status = ShellSessionStatus::Closed;
        state.active_exec_id = None;
        state.updated_at = Utc::now();

        Ok(json!({
            "session_id": session_id,
            "closed": true,
        }))
    }

    async fn interrupt_session(&self, session_id: &str) -> Result<Value> {
        let handle = {
            let locked = self.sessions.lock().expect("shell sessions lock poisoned");
            let handle = locked
                .get(session_id)
                .ok_or_else(|| anyhow!("shell session not found: {session_id}"))?;
            (
                Arc::clone(&handle.state),
                Arc::clone(&handle.child),
                handle.process_group_id,
                handle.pty,
            )
        };
        let (state, child, process_group_id, pty) = handle;
        self.refresh_session_process(&state, &child).await?;
        let active_exec_id = {
            let mut locked = state.lock().expect("shell session state lock poisoned");
            let Some(exec_id) = locked.active_exec_id.clone() else {
                return Ok(json!({
                    "session_id": session_id,
                    "signaled": false,
                    "signal": "SIGINT",
                    "reason": "no active execution",
                }));
            };
            if let Some(active) = locked.active_execution.as_mut() {
                active.interrupted = true;
            }
            exec_id
        };

        if pty {
            let mut process = child.lock().await;
            let stdin = process
                .stdin
                .as_mut()
                .ok_or_else(|| anyhow!("shell session stdin is unavailable"))?;
            stdin.write_all(&[3]).await?;
            stdin.flush().await?;
            if process_group_id > 0 {
                let _ = signal_process_group(process_group_id, libc::SIGINT);
            }
        } else if process_group_id > 0 {
            signal_process_group(process_group_id, libc::SIGINT)?;
        } else {
            bail!("shell session cannot be interrupted");
        }

        Ok(json!({
            "session_id": session_id,
            "signaled": true,
            "signal": "SIGINT",
            "exec_id": active_exec_id,
        }))
    }

    async fn resize_session(&self, session_id: &str, columns: u16, rows: u16) -> Result<Value> {
        if columns == 0 || rows == 0 {
            bail!("shell_session_resize requires positive cols and rows");
        }
        let (state, child) = self.session_refs(session_id)?;
        self.refresh_session_process(&state, &child).await?;

        {
            let mut locked = state.lock().expect("shell session state lock poisoned");
            if !locked.pty {
                return Ok(json!({
                    "session_id": session_id,
                    "resized": false,
                    "pty": false,
                    "cols": locked.columns,
                    "rows": locked.rows,
                }));
            }
            locked.columns = columns;
            locked.rows = rows;
            locked.updated_at = Utc::now();
        }

        self.apply_resize_command(session_id, columns, rows).await?;

        Ok(json!({
            "session_id": session_id,
            "resized": true,
            "pty": true,
            "cols": columns,
            "rows": rows,
        }))
    }

    async fn apply_resize_command(&self, session_id: &str, columns: u16, rows: u16) -> Result<()> {
        let (_, child) = self.session_refs(session_id)?;
        let mut process = child.lock().await;
        let stdin = process
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("shell session stdin is unavailable"))?;
        let script = format!("stty cols {columns} rows {rows} >/dev/null 2>&1 || true\n");
        stdin.write_all(script.as_bytes()).await?;
        stdin.flush().await?;
        Ok(())
    }

    fn session_refs(&self, session_id: &str) -> Result<(SessionStateRef, SessionChildRef)> {
        let locked = self.sessions.lock().expect("shell sessions lock poisoned");
        let handle = locked
            .get(session_id)
            .ok_or_else(|| anyhow!("shell session not found: {session_id}"))?;
        Ok((Arc::clone(&handle.state), Arc::clone(&handle.child)))
    }

    async fn refresh_session_process(
        &self,
        state: &Arc<Mutex<ShellSessionState>>,
        child: &Arc<AsyncMutex<Child>>,
    ) -> Result<()> {
        let mut process = child.lock().await;
        if let Some(status) = process.try_wait()? {
            let mut locked = state.lock().expect("shell session state lock poisoned");
            locked.last_exit_code = status.code();
            if let Some(mut active) = locked.active_execution.take() {
                active.exit_code = status.code();
                active.finished_at = Some(Utc::now());
                active.status = if active.interrupted {
                    ShellExecutionStatus::Interrupted
                } else if status.success() {
                    ShellExecutionStatus::Completed
                } else {
                    ShellExecutionStatus::Failed
                };
                locked.pending_completed_execution = Some(active.clone());
                locked.last_execution = Some(active);
            }
            locked.active_exec_id = None;
            locked.status = if status.success() {
                ShellSessionStatus::Closed
            } else {
                ShellSessionStatus::Failed
            };
            locked.updated_at = Utc::now();
        }
        Ok(())
    }
}

fn spawn_output_pump<T>(stream: T, state: Arc<Mutex<ShellSessionState>>, stream_name: &'static str)
where
    T: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            let bytes = match reader.read_line(&mut line).await {
                Ok(bytes) => bytes,
                Err(error) => {
                    let mut locked = state.lock().expect("shell session state lock poisoned");
                    locked.status = ShellSessionStatus::Failed;
                    locked
                        .append_chunk(stream_name, format!("shell stream read failed: {error}\n"));
                    break;
                }
            };
            if bytes == 0 {
                break;
            }

            if stream_name == "stdout" {
                if line.starts_with(EXEC_START_MARKER) {
                    continue;
                }
                if let Some((exec_id, exit_code, cwd)) = parse_exec_end_marker(&line) {
                    let mut locked = state.lock().expect("shell session state lock poisoned");
                    locked.finish_exec(&exec_id, exit_code, cwd);
                    continue;
                }
            }

            let mut locked = state.lock().expect("shell session state lock poisoned");
            locked.append_chunk(stream_name, line.clone());
        }
    });
}

fn parse_exec_end_marker(line: &str) -> Option<(String, i32, String)> {
    let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
    let rest = trimmed.strip_prefix(EXEC_END_MARKER)?.strip_prefix('\t')?;
    let mut parts = rest.splitn(3, '\t');
    let exec_id = parts.next()?.to_string();
    let exit_code = parts.next()?.parse::<i32>().ok()?;
    let cwd = parts.next()?.to_string();
    Some((exec_id, exit_code, cwd))
}

fn build_session_exec_script(
    exec_id: &str,
    command: &str,
    timeout_secs: u64,
    detach: bool,
) -> String {
    let maybe_background = if detach { " &" } else { "" };
    format!(
        "printf '%s\\n' '{start}{exec_id}'\n{{\n{command}\n}}{background}\n__ajx_status=$?\nprintf '{end}\\t%s\\t%s\\t%s\\n' '{exec_id}' \"$__ajx_status\" \"$PWD\"\n",
        start = EXEC_START_MARKER,
        end = EXEC_END_MARKER,
        background = maybe_background,
        exec_id = exec_id,
        command = wrap_timeout(command, timeout_secs),
    )
}

fn wrap_timeout(command: &str, timeout_secs: u64) -> String {
    if timeout_secs == 0 {
        return command.to_string();
    }
    format!(
        "if command -v timeout >/dev/null 2>&1; then\n  timeout {timeout_secs}s sh -lc {quoted}\nelse\n  {command}\nfi",
        timeout_secs = timeout_secs,
        quoted = shell_single_quote(command),
        command = command,
    )
}

fn resolve_shell(requested: Option<&str>) -> String {
    requested
        .map(str::to_string)
        .or_else(|| env::var("SHELL").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_SHELL.to_string())
}

fn resolve_cwd(requested: Option<&str>) -> Result<PathBuf> {
    match requested {
        Some(path) => Ok(PathBuf::from(path)),
        None => env::current_dir().context("failed to resolve current working directory"),
    }
}

fn apply_env(command: &mut Command, env_map: &Map<String, Value>) -> Result<()> {
    for (key, value) in env_map {
        let value = value
            .as_str()
            .ok_or_else(|| anyhow!("shell env values must be strings"))?;
        command.env(key, value);
    }
    Ok(())
}

fn clip_text_bytes(text: &str, max_bytes: usize) -> (String, bool) {
    if text.len() <= max_bytes {
        return (text.to_string(), false);
    }
    let mut used = 0;
    let mut clipped = String::new();
    for ch in text.chars() {
        let width = ch.len_utf8();
        if used + width > max_bytes {
            break;
        }
        used += width;
        clipped.push(ch);
    }
    (clipped, true)
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn signal_process_group(process_group_id: i32, signal: i32) -> Result<()> {
    let rc = unsafe { libc::kill(-process_group_id, signal) };
    if rc == 0 {
        Ok(())
    } else {
        Err(anyhow!(
            "failed to signal process group {process_group_id}: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ShellExecToolPlugin;

#[derive(Debug, Clone, Default)]
pub struct ShellSessionOpenToolPlugin;

#[derive(Debug, Clone, Default)]
pub struct ShellSessionExecToolPlugin;

#[derive(Debug, Clone, Default)]
pub struct ShellSessionReadToolPlugin;

#[derive(Debug, Clone, Default)]
pub struct ShellSessionListToolPlugin;

#[derive(Debug, Clone, Default)]
pub struct ShellSessionCloseToolPlugin;

#[derive(Debug, Clone, Default)]
pub struct ShellSessionInterruptToolPlugin;

#[derive(Debug, Clone, Default)]
pub struct ShellSessionResizeToolPlugin;

fn shell_manifest(id: &str) -> PluginManifest {
    PluginManifest {
        id: id.into(),
        version: "0.1.0".into(),
        capabilities: vec![PluginCapability::Tool(ToolCapability::Tool)],
        config_schema: None,
        required_permissions: vec![Permission::ReadWorkspace, Permission::WriteWorkspace],
        dependencies: Vec::new(),
        optional_dependencies: Vec::new(),
        provided_resources: Vec::new(),
        hooks: Vec::new(),
    }
}

#[async_trait]
impl Plugin for ShellExecToolPlugin {
    fn manifest(&self) -> PluginManifest {
        shell_manifest("tool.builtin.shell_exec")
    }
}

#[async_trait]
impl Plugin for ShellSessionOpenToolPlugin {
    fn manifest(&self) -> PluginManifest {
        shell_manifest("tool.builtin.shell_session_open")
    }
}

#[async_trait]
impl Plugin for ShellSessionExecToolPlugin {
    fn manifest(&self) -> PluginManifest {
        shell_manifest("tool.builtin.shell_session_exec")
    }
}

#[async_trait]
impl Plugin for ShellSessionReadToolPlugin {
    fn manifest(&self) -> PluginManifest {
        shell_manifest("tool.builtin.shell_session_read")
    }
}

#[async_trait]
impl Plugin for ShellSessionListToolPlugin {
    fn manifest(&self) -> PluginManifest {
        shell_manifest("tool.builtin.shell_session_list")
    }
}

#[async_trait]
impl Plugin for ShellSessionCloseToolPlugin {
    fn manifest(&self) -> PluginManifest {
        shell_manifest("tool.builtin.shell_session_close")
    }
}

#[async_trait]
impl Plugin for ShellSessionInterruptToolPlugin {
    fn manifest(&self) -> PluginManifest {
        shell_manifest("tool.builtin.shell_session_interrupt")
    }
}

#[async_trait]
impl Plugin for ShellSessionResizeToolPlugin {
    fn manifest(&self) -> PluginManifest {
        shell_manifest("tool.builtin.shell_session_resize")
    }
}

#[async_trait]
impl ToolPlugin for ShellExecToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "shell_exec".into(),
            description: "Run a one-shot shell command without preserving context.".into(),
            when_to_use: "Use for a single command such as pwd, ls, git status, or cargo check."
                .into(),
            when_not_to_use: "Do not use when later commands must reuse cd/export/source state."
                .into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" },
                    "cwd": { "type": "string" },
                    "env": { "type": "object", "additionalProperties": { "type": "string" } },
                    "shell": { "type": "string" },
                    "timeout_secs": { "type": "integer" },
                    "capture_limit": { "type": "integer" }
                },
                "required": ["command"]
            }),
            default_timeout_secs: 120,
            idempotent: false,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        let command = required_arg_str(&call.args, "command", "shell_exec")?;
        let env_map = optional_object(&call.args, "env")?;
        let value = manager()
            .stateless_exec(
                command,
                support::parse_optional_string(&call.args, "cwd"),
                &env_map,
                support::parse_optional_string(&call.args, "shell"),
                support::parse_optional_usize(&call.args, "timeout_secs")?.map(|v| v as u64),
                support::parse_optional_usize(&call.args, "capture_limit")?,
            )
            .await?;
        support::json_tool_output(value)
    }
}

#[async_trait]
impl ToolPlugin for ShellSessionOpenToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "shell_session_open".into(),
            description: "Open a persistent shell session that retains cwd and environment."
                .into(),
            when_to_use:
                "Use when later commands need the same shell context or when long tasks must be observed."
                    .into(),
            when_not_to_use: "Do not use for a simple one-shot command.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "cwd": { "type": "string" },
                    "env": { "type": "object", "additionalProperties": { "type": "string" } },
                    "shell": { "type": "string" },
                    "pty": { "type": "boolean" },
                    "title": { "type": "string" }
                }
            }),
            default_timeout_secs: 30,
            idempotent: false,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        let env_map = optional_object(&call.args, "env")?;
        let value = manager()
            .open_session(
                support::parse_optional_string(&call.args, "cwd"),
                &env_map,
                support::parse_optional_string(&call.args, "shell"),
                support::parse_optional_bool(&call.args, "pty").unwrap_or(false),
                support::parse_optional_string(&call.args, "title").map(str::to_string),
            )
            .await?;
        support::json_tool_output(value)
    }
}

#[async_trait]
impl ToolPlugin for ShellSessionExecToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "shell_session_exec".into(),
            description: "Execute a command inside an existing persistent shell session.".into(),
            when_to_use:
                "Use after shell_session_open when commands depend on prior cd/export/source state."
                    .into(),
            when_not_to_use: "Do not use without a valid shell session id.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "command": { "type": "string" },
                    "timeout_secs": { "type": "integer" },
                    "detach": { "type": "boolean" }
                },
                "required": ["session_id", "command"]
            }),
            default_timeout_secs: 300,
            idempotent: false,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        let value = manager()
            .session_exec(
                required_arg_str(&call.args, "session_id", "shell_session_exec")?,
                required_arg_str(&call.args, "command", "shell_session_exec")?,
                support::parse_optional_usize(&call.args, "timeout_secs")?.map(|v| v as u64),
                support::parse_optional_bool(&call.args, "detach").unwrap_or(false),
            )
            .await?;
        support::json_tool_output(value)
    }
}

#[async_trait]
impl ToolPlugin for ShellSessionReadToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "shell_session_read".into(),
            description: "Read new stdout/stderr output and execution status from a shell session."
                .into(),
            when_to_use: "Use after shell_session_exec to poll output incrementally.".into(),
            when_not_to_use: "Do not use as a substitute for re-running commands.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "since_seq": { "type": "integer" },
                    "max_bytes": { "type": "integer" }
                },
                "required": ["session_id"]
            }),
            default_timeout_secs: 30,
            idempotent: true,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        let value = manager()
            .session_read(
                required_arg_str(&call.args, "session_id", "shell_session_read")?,
                support::parse_optional_usize(&call.args, "since_seq")?.unwrap_or(0) as u64,
                support::parse_optional_usize(&call.args, "max_bytes")?
                    .unwrap_or(DEFAULT_SESSION_READ_LIMIT),
            )
            .await?;
        support::json_tool_output(value)
    }
}

#[async_trait]
impl ToolPlugin for ShellSessionListToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "shell_session_list".into(),
            description: "List currently open shell sessions.".into(),
            when_to_use: "Use to inspect available shell sessions before reusing one.".into(),
            when_not_to_use: "Do not use when you already know the target session id.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {}
            }),
            default_timeout_secs: 10,
            idempotent: true,
        }
    }

    async fn invoke(&self, _call: &ToolCall) -> Result<super::ToolOutput> {
        support::json_tool_output(manager().list_sessions().await?)
    }
}

#[async_trait]
impl ToolPlugin for ShellSessionCloseToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "shell_session_close".into(),
            description: "Close a shell session and free its resources.".into(),
            when_to_use: "Use when a shell session is no longer needed.".into(),
            when_not_to_use: "Do not use while you still need the session state or output.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "force": { "type": "boolean" }
                },
                "required": ["session_id"]
            }),
            default_timeout_secs: 30,
            idempotent: false,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        let value = manager()
            .close_session(
                required_arg_str(&call.args, "session_id", "shell_session_close")?,
                support::parse_optional_bool(&call.args, "force").unwrap_or(false),
            )
            .await?;
        support::json_tool_output(value)
    }
}

#[async_trait]
impl ToolPlugin for ShellSessionInterruptToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "shell_session_interrupt".into(),
            description:
                "Send SIGINT or terminal interrupt to the active command in a shell session.".into(),
            when_to_use: "Use when a session command is stuck or needs Ctrl-C.".into(),
            when_not_to_use: "Do not use when the session has no active execution.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" }
                },
                "required": ["session_id"]
            }),
            default_timeout_secs: 10,
            idempotent: false,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        support::json_tool_output(
            manager()
                .interrupt_session(required_arg_str(
                    &call.args,
                    "session_id",
                    "shell_session_interrupt",
                )?)
                .await?,
        )
    }
}

#[async_trait]
impl ToolPlugin for ShellSessionResizeToolPlugin {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "shell_session_resize".into(),
            description: "Resize the terminal dimensions of a PTY-backed shell session.".into(),
            when_to_use: "Use when terminal-aware programs need updated cols and rows.".into(),
            when_not_to_use: "Do not use on non-PTY shell sessions.".into(),
            arguments_schema: json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "cols": { "type": "integer" },
                    "rows": { "type": "integer" }
                },
                "required": ["session_id", "cols", "rows"]
            }),
            default_timeout_secs: 10,
            idempotent: false,
        }
    }

    async fn invoke(&self, call: &ToolCall) -> Result<super::ToolOutput> {
        support::json_tool_output(
            manager()
                .resize_session(
                    required_arg_str(&call.args, "session_id", "shell_session_resize")?,
                    support::parse_required_usize(&call.args, "cols", "shell_session_resize")?
                        as u16,
                    support::parse_required_usize(&call.args, "rows", "shell_session_resize")?
                        as u16,
                )
                .await?,
        )
    }
}

fn required_arg_str<'a>(args: &'a Value, key: &str, tool_name: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| anyhow!("{tool_name} requires args.{key}"))
}

fn optional_object(args: &Value, key: &str) -> Result<Map<String, Value>> {
    match args.get(key) {
        Some(Value::Object(map)) => Ok(map.clone()),
        Some(_) => bail!("args.{key} must be an object"),
        None => Ok(Map::new()),
    }
}
