use anyhow::{anyhow, Context, Result};
use core_agentic::runtime::protocol::{
    InitOverrides, ProtocolEvent, ProtocolRequest, Request, PROTOCOL_VERSION,
};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

pub struct RuntimeClient {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    sequence: u64,
}

impl RuntimeClient {
    pub async fn spawn(overrides: InitOverrides) -> Result<Self> {
        let binary = runtime_binary()?;
        Self::spawn_binary(&binary, overrides).await
    }

    pub async fn spawn_binary(binary: &Path, overrides: InitOverrides) -> Result<Self> {
        let mut child = Command::new(binary)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("failed to spawn {}", binary.display()))?;
        let stdin = child.stdin.take().context("runtime stdin unavailable")?;
        let stdout = child.stdout.take().context("runtime stdout unavailable")?;
        let mut client = Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            sequence: 0,
        };

        let ready = client.next_event().await?;
        if !matches!(ready.event, core_agentic::Event::Ready { .. }) {
            return Err(anyhow!("runtime did not emit ready"));
        }
        client.send(Request::Init { overrides }).await?;
        let initialized = client.next_event().await?;
        match initialized.event {
            core_agentic::Event::InitOk { .. } => Ok(client),
            core_agentic::Event::Error { message } => Err(anyhow!(message)),
            other => Err(anyhow!("unexpected initialization event: {other:?}")),
        }
    }

    pub async fn send(&mut self, request: Request) -> Result<String> {
        self.sequence += 1;
        let id = format!("cli-{}", self.sequence);
        let envelope = ProtocolRequest::new(id.clone(), request);
        let mut bytes = serde_json::to_vec(&envelope)?;
        bytes.push(b'\n');
        self.stdin.write_all(&bytes).await?;
        self.stdin.flush().await?;
        Ok(id)
    }

    pub async fn next_event(&mut self) -> Result<ProtocolEvent> {
        let line = self
            .lines
            .next_line()
            .await?
            .ok_or_else(|| anyhow!("runtime stdout closed"))?;
        let event: ProtocolEvent = serde_json::from_str(&line)
            .with_context(|| format!("invalid runtime event: {line}"))?;
        if event.v != PROTOCOL_VERSION {
            return Err(anyhow!("unsupported runtime protocol version: {}", event.v));
        }
        Ok(event)
    }

    pub async fn send_and_wait(&mut self, request: Request) -> Result<ProtocolEvent> {
        let id = self.send(request).await?;
        loop {
            let event = self.next_event().await?;
            if event.request_id.as_deref() == Some(&id) {
                return Ok(event);
            }
        }
    }

    pub async fn run<C, E, R>(
        &mut self,
        task: String,
        attachments: Vec<core_agentic::Attachment>,
        mut on_chunk: C,
        mut on_event: E,
        mut responder: R,
    ) -> Result<String>
    where
        C: FnMut(&str),
        E: FnMut(core_agentic::Event),
        R: FnMut(&ProtocolEvent) -> Option<Request>,
    {
        let request_id = self.send(Request::Run { task, attachments }).await?;
        let mut streamed = String::new();
        loop {
            let event = self.next_event().await?;
            if event.request_id.as_deref() != Some(&request_id) {
                continue;
            }
            if let Some(response) = responder(&event) {
                self.send(response).await?;
            }
            match event.event {
                core_agentic::Event::AssistantDelta { content } => {
                    streamed.push_str(&content);
                    on_chunk(&content);
                }
                core_agentic::Event::Done { result } => {
                    return Ok(if result.is_empty() { streamed } else { result });
                }
                core_agentic::Event::Error { message } => return Err(anyhow!(message)),
                event => on_event(event),
            }
        }
    }

    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.send(Request::Shutdown).await?;
        self.stdin.shutdown().await?;
        let _ = self.child.wait().await?;
        Ok(())
    }
}

pub fn runtime_binary() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("AGENTIC_RUNTIME_BIN") {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe()?;
    let sibling = current.with_file_name(if cfg!(windows) {
        "agentic-runtime.exe"
    } else {
        "agentic-runtime"
    });
    if sibling.exists() {
        return Ok(sibling);
    }
    for ancestor in current.ancestors() {
        let candidate = ancestor.join(if cfg!(windows) {
            "agentic-runtime.exe"
        } else {
            "agentic-runtime"
        });
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(anyhow!(
        "agentic-runtime not found next to {}; set AGENTIC_RUNTIME_BIN",
        current.display()
    ))
}
