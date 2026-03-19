use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::Notify;
use tracing::{info, warn};

const LISTENER_RETRY_DELAY: Duration = Duration::from_secs(2);

#[derive(Debug, Clone)]
pub struct ListenOptions {
    pub api_port: u16,
    pub listener_label: String,
    pub send_as: String,
    pub wait_timeout_secs: u64,
    pub once: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct AgentConfigFile {
    backend: Option<String>,
    command: Option<String>,
    model: Option<String>,
    ollama_url: Option<String>,
    project_dir: Option<String>,
}

#[derive(Debug, Clone)]
enum Backend {
    Claude,
    Codex,
    OpenAi,
    Ollama,
    Custom,
}

#[derive(Debug, Clone)]
struct AgentRuntimeConfig {
    backend: Backend,
    command: Option<String>,
    model: Option<String>,
    ollama_url: String,
    project_dir: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RegisterConsumerResponse {
    consumer_id: u64,
}

#[derive(Debug, Serialize)]
struct RegisterConsumerRequest<'a> {
    label: &'a str,
    suppress_wake: bool,
}

#[derive(Debug, Deserialize, Clone)]
struct InboxMessage {
    id: String,
    from: String,
    body: String,
    #[allow(dead_code)]
    timestamp: String,
    conversation_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct SendRequest<'a> {
    body: &'a str,
    to: Option<&'a str>,
    from: Option<&'a str>,
    reply_to: Option<&'a str>,
    conversation_id: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct RoomSendRequest<'a> {
    body: &'a str,
    from: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct ProjectsResponse {
    projects: Vec<ProjectSummary>,
}

#[derive(Debug, Deserialize)]
struct ProjectSummary {
    id: String,
}

#[derive(Debug, Deserialize)]
struct RoomsResponse {
    rooms: Vec<RoomSummary>,
}

#[derive(Debug, Deserialize)]
struct RoomSummary {
    room_id: String,
    name: String,
    conversation_id: String,
}

#[derive(Debug, Clone)]
struct RoomRoute {
    project_id: String,
    room_id: String,
    room_name: String,
}

#[derive(Debug, Clone)]
struct MessageBatch {
    from: String,
    conversation_id: Option<String>,
    reply_to: Option<String>,
    messages: Vec<InboxMessage>,
}

#[derive(Default)]
struct ConversationRouteCache {
    routes: HashMap<String, RoomRoute>,
    last_refresh: Option<Instant>,
}

enum PollOutcome {
    Messages(Vec<InboxMessage>),
    Reaped,
}

pub async fn listen(options: ListenOptions) -> Result<()> {
    let cfg = AgentRuntimeConfig::load()?;
    std::env::set_current_dir(&cfg.project_dir).with_context(|| {
        format!(
            "failed to switch to configured project directory {}",
            cfg.project_dir.display()
        )
    })?;

    let api_base = format!("http://127.0.0.1:{}", options.api_port);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(
            options.wait_timeout_secs.saturating_add(5),
        ))
        .build()
        .context("failed to build HTTP client")?;

    info!(
        "Starting child-agent listener '{}' (send_as='{}', backend={:?})",
        options.listener_label, options.send_as, cfg.backend
    );

    let mut listener_id =
        register_consumer_with_retry(&client, &api_base, &options.listener_label).await?;
    let mut routes = ConversationRouteCache::default();

    loop {
        let outcome = tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Child-agent listener received Ctrl+C, shutting down");
                break;
            }
            outcome = poll_messages(&client, &api_base, listener_id, options.wait_timeout_secs) => {
                match outcome {
                    Ok(outcome) => outcome,
                    Err(err) => {
                        warn!(
                            "Child-agent listener consumer {} poll failed: {}. Re-registering listener '{}'",
                            listener_id, err, options.listener_label
                        );
                        listener_id = register_consumer_with_retry(
                            &client,
                            &api_base,
                            &options.listener_label,
                        )
                        .await?;
                        continue;
                    }
                }
            }
        };

        let messages = match outcome {
            PollOutcome::Messages(messages) => messages,
            PollOutcome::Reaped => {
                warn!(
                    "Child-agent listener consumer {} was reaped, registering again",
                    listener_id
                );
                listener_id =
                    register_consumer_with_retry(&client, &api_base, &options.listener_label)
                        .await?;
                continue;
            }
        };

        if messages.is_empty() {
            if options.once {
                break;
            }
            continue;
        }

        for batch in build_batches(messages) {
            if is_self_message(&batch.from, &options.send_as, &options.listener_label) {
                continue;
            }

            let route = match batch.conversation_id.as_deref() {
                Some(conversation_id) => {
                    routes.resolve(&client, &api_base, conversation_id).await?
                }
                None => None,
            };

            let heartbeat_stop = Arc::new(Notify::new());
            let heartbeat_task = tokio::spawn(run_processing_heartbeat(
                client.clone(),
                api_base.clone(),
                listener_id,
                heartbeat_stop.clone(),
            ));

            let reply = match call_agent(&client, &cfg, &options, &batch, route.as_ref()).await {
                Ok(reply) => reply,
                Err(err) => {
                    heartbeat_stop.notify_waiters();
                    let _ = heartbeat_task.await;
                    warn!(
                        "Child-agent backend failed for message from {}: {}",
                        batch.from, err
                    );
                    continue;
                }
            };

            heartbeat_stop.notify_waiters();
            let _ = heartbeat_task.await;

            let Some(reply) = normalize_reply(&reply, &options.send_as) else {
                continue;
            };

            if let Some(route) = route {
                if let Err(err) =
                    send_room_reply(&client, &api_base, &route, &options.send_as, &reply).await
                {
                    warn!(
                        "Failed to send room reply to {} / {}: {}",
                        route.project_id, route.room_name, err
                    );
                }
            } else if let Err(err) = send_direct_reply(
                &client,
                &api_base,
                &batch.from,
                &options.send_as,
                batch.reply_to.as_deref(),
                batch.conversation_id.as_deref(),
                &reply,
            )
            .await
            {
                warn!("Failed to send direct reply to {}: {}", batch.from, err);
            }
        }

        if options.once {
            break;
        }
    }

    if let Err(err) = unregister_consumer(&client, &api_base, listener_id).await {
        warn!(
            "Failed to unregister child-agent listener consumer {}: {}",
            listener_id, err
        );
    }

    Ok(())
}

async fn register_consumer_with_retry(
    client: &reqwest::Client,
    api_base: &str,
    label: &str,
) -> Result<u64> {
    register_consumer_with_retry_and_delay(client, api_base, label, LISTENER_RETRY_DELAY).await
}

async fn register_consumer_with_retry_and_delay(
    client: &reqwest::Client,
    api_base: &str,
    label: &str,
    retry_delay: Duration,
) -> Result<u64> {
    loop {
        match register_consumer(client, api_base, label, true).await {
            Ok(id) => {
                info!(
                    "Child-agent listener '{}' registered consumer {}",
                    label, id
                );
                return Ok(id);
            }
            Err(err) => {
                warn!(
                    "Failed to register child-agent listener '{}': {}. Retrying in {}s",
                    label,
                    err,
                    retry_delay.as_secs()
                );
                tokio::time::sleep(retry_delay).await;
            }
        }
    }
}

impl AgentRuntimeConfig {
    fn load() -> Result<Self> {
        let config_path = default_agent_config_path();
        let file_cfg = if config_path.exists() {
            let raw = std::fs::read_to_string(&config_path)
                .with_context(|| format!("failed to read {}", config_path.display()))?;
            toml::from_str::<AgentConfigFile>(&raw)
                .with_context(|| format!("failed to parse {}", config_path.display()))?
        } else {
            AgentConfigFile::default()
        };

        let backend = std::env::var("AGORA_AGENT_BACKEND")
            .ok()
            .or(file_cfg.backend)
            .unwrap_or_else(|| "claude".to_string());
        let backend = parse_backend(&backend)?;

        let project_dir = std::env::var("AGORA_PROJECT_DIR")
            .ok()
            .or(file_cfg.project_dir)
            .map(PathBuf::from)
            .unwrap_or(std::env::current_dir().context("failed to resolve current directory")?);

        Ok(Self {
            backend,
            command: std::env::var("AGORA_AGENT_COMMAND")
                .ok()
                .or(file_cfg.command),
            model: file_cfg.model,
            ollama_url: file_cfg
                .ollama_url
                .unwrap_or_else(|| "http://localhost:11434".to_string()),
            project_dir,
        })
    }
}

impl ConversationRouteCache {
    async fn resolve(
        &mut self,
        client: &reqwest::Client,
        api_base: &str,
        conversation_id: &str,
    ) -> Result<Option<RoomRoute>> {
        let should_refresh = self
            .last_refresh
            .is_none_or(|ts| ts.elapsed() > Duration::from_secs(30))
            || !self.routes.contains_key(conversation_id);
        if should_refresh {
            self.refresh(client, api_base).await?;
        }
        Ok(self.routes.get(conversation_id).cloned())
    }

    async fn refresh(&mut self, client: &reqwest::Client, api_base: &str) -> Result<()> {
        let projects = client
            .get(format!("{}/projects", api_base))
            .send()
            .await
            .context("failed to fetch project list")?
            .error_for_status()
            .context("project list request failed")?
            .json::<ProjectsResponse>()
            .await
            .context("failed to decode project list")?;

        let mut routes = HashMap::new();
        for project in projects.projects {
            let rooms = client
                .get(format!("{}/projects/{}/rooms", api_base, project.id))
                .send()
                .await
                .with_context(|| format!("failed to fetch rooms for project {}", project.id))?
                .error_for_status()
                .with_context(|| format!("room list request failed for project {}", project.id))?
                .json::<RoomsResponse>()
                .await
                .with_context(|| format!("failed to decode rooms for project {}", project.id))?;
            for room in rooms.rooms {
                routes.insert(
                    room.conversation_id.clone(),
                    RoomRoute {
                        project_id: project.id.clone(),
                        room_id: room.room_id,
                        room_name: room.name,
                    },
                );
            }
        }

        self.routes = routes;
        self.last_refresh = Some(Instant::now());
        Ok(())
    }
}

fn default_agent_config_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".agora").join("agent.toml")
}

fn parse_backend(name: &str) -> Result<Backend> {
    match name.to_ascii_lowercase().as_str() {
        "claude" => Ok(Backend::Claude),
        "codex" => Ok(Backend::Codex),
        "openai" => Ok(Backend::OpenAi),
        "ollama" => Ok(Backend::Ollama),
        "custom" => Ok(Backend::Custom),
        other => bail!("unsupported agent backend '{}'", other),
    }
}

fn build_batches(messages: Vec<InboxMessage>) -> Vec<MessageBatch> {
    let mut batches: Vec<MessageBatch> = Vec::new();
    for message in messages {
        match batches.last_mut() {
            Some(current)
                if current.from == message.from
                    && current.conversation_id == message.conversation_id =>
            {
                current.reply_to = Some(message.id.clone());
                current.messages.push(message);
            }
            _ => {
                batches.push(MessageBatch {
                    from: message.from.clone(),
                    conversation_id: message.conversation_id.clone(),
                    reply_to: Some(message.id.clone()),
                    messages: vec![message],
                });
            }
        }
    }
    batches
}

fn is_self_message(from: &str, send_as: &str, listener_label: &str) -> bool {
    from == send_as || from == listener_label || from.starts_with(&format!("{}-", send_as))
}

async fn register_consumer(
    client: &reqwest::Client,
    api_base: &str,
    label: &str,
    suppress_wake: bool,
) -> Result<u64> {
    let response = client
        .post(format!("{}/consumers", api_base))
        .json(&RegisterConsumerRequest {
            label,
            suppress_wake,
        })
        .send()
        .await
        .with_context(|| format!("failed to register consumer '{}'", label))?
        .error_for_status()
        .with_context(|| format!("consumer registration failed for '{}'", label))?
        .json::<RegisterConsumerResponse>()
        .await
        .context("failed to decode consumer registration response")?;
    Ok(response.consumer_id)
}

async fn unregister_consumer(
    client: &reqwest::Client,
    api_base: &str,
    consumer_id: u64,
) -> Result<()> {
    let response = client
        .delete(format!("{}/consumers/{}", api_base, consumer_id))
        .send()
        .await
        .with_context(|| format!("failed to unregister consumer {}", consumer_id))?;
    if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(());
    }
    bail!(
        "consumer unregister failed with status {}",
        response.status()
    )
}

async fn poll_messages(
    client: &reqwest::Client,
    api_base: &str,
    consumer_id: u64,
    wait_timeout_secs: u64,
) -> Result<PollOutcome> {
    let response = client
        .get(format!("{}/consumers/{}/messages", api_base, consumer_id))
        .query(&[
            ("wait", "true"),
            ("timeout", &wait_timeout_secs.min(120).to_string()),
        ])
        .send()
        .await
        .with_context(|| format!("failed to poll consumer {}", consumer_id))?;

    match response.status() {
        reqwest::StatusCode::OK => {
            let messages = response
                .json::<Vec<InboxMessage>>()
                .await
                .context("failed to decode polled messages")?;
            Ok(PollOutcome::Messages(messages))
        }
        reqwest::StatusCode::NOT_FOUND => Ok(PollOutcome::Reaped),
        reqwest::StatusCode::TOO_MANY_REQUESTS => {
            tokio::time::sleep(Duration::from_millis(250)).await;
            Ok(PollOutcome::Messages(Vec::new()))
        }
        status => bail!("consumer poll failed with status {}", status),
    }
}

async fn call_agent(
    client: &reqwest::Client,
    cfg: &AgentRuntimeConfig,
    options: &ListenOptions,
    batch: &MessageBatch,
    route: Option<&RoomRoute>,
) -> Result<String> {
    let system_prompt = if let Some(route) = route {
        format!(
            "This is a chat between AI agents on the Agora network. You are {}. \
             The conversation is happening in the '{}' room of project {}. \
             Write {}'s next message in the conversation. Be specific and concise. \
             Output only the message text.",
            options.send_as, route.room_name, route.project_id, options.send_as
        )
    } else {
        format!(
            "This is a chat between AI agents on the Agora network. You are {}. \
             Write {}'s next message in the conversation. Be specific and concise. \
             Output only the message text.",
            options.send_as, options.send_as
        )
    };
    let user_prompt = format!(
        "[{}]: {}\n\n[{}]:",
        batch.from,
        joined_bodies(&batch.messages),
        options.send_as
    );

    match cfg.backend {
        Backend::Claude => run_claude(&cfg.project_dir, &system_prompt, &user_prompt).await,
        Backend::Codex => run_codex(cfg, &system_prompt, &user_prompt).await,
        Backend::OpenAi => run_openai(client, cfg, &system_prompt, &user_prompt).await,
        Backend::Ollama => run_ollama(client, cfg, &system_prompt, &user_prompt).await,
        Backend::Custom => run_custom(cfg, &user_prompt).await,
    }
}

async fn run_claude(project_dir: &Path, system_prompt: &str, user_prompt: &str) -> Result<String> {
    let mut cmd = Command::new(claude_binary());
    cmd.arg("-p")
        .arg(user_prompt)
        .arg("--tools")
        .arg("")
        .arg("--system-prompt")
        .arg(system_prompt)
        .current_dir(project_dir)
        .env_remove("CLAUDE_CODE")
        .env_remove("CLAUDECODE")
        .env_remove("CLAUDE_CODE_SSE_PORT")
        .env_remove("CLAUDE_CODE_ENTRYPOINT");
    let output = cmd
        .output()
        .await
        .context("failed to launch claude backend")?;
    if !output.status.success() {
        bail!(
            "claude backend exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

async fn run_codex(
    cfg: &AgentRuntimeConfig,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String> {
    let output_path =
        std::env::temp_dir().join(format!("agora-codex-{}.txt", uuid::Uuid::new_v4()));
    let prompt = format!("{system_prompt}\n\n{user_prompt}");

    let mut cmd = Command::new(codex_binary());
    cmd.arg("exec")
        .arg("--dangerously-bypass-approvals-and-sandbox")
        .arg("--skip-git-repo-check")
        .arg("--color")
        .arg("never")
        .arg("--ephemeral")
        .arg("-C")
        .arg(&cfg.project_dir)
        .arg("-o")
        .arg(&output_path);
    if let Some(model) = cfg.model.as_deref() {
        cmd.arg("-m").arg(model);
    }
    cmd.arg(&prompt);

    let output = cmd
        .output()
        .await
        .context("failed to launch codex backend")?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&output_path);
        bail!(
            "codex backend exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let reply = std::fs::read_to_string(&output_path)
        .with_context(|| format!("failed to read codex output {}", output_path.display()))?;
    let _ = std::fs::remove_file(&output_path);
    Ok(reply)
}

async fn run_openai(
    client: &reqwest::Client,
    cfg: &AgentRuntimeConfig,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String> {
    let api_key = std::env::var("OPENAI_API_KEY").context("OPENAI_API_KEY is not set")?;
    let model = cfg.model.as_deref().unwrap_or("gpt-4o");
    let payload = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "max_tokens": 1024
    });
    let body: serde_json::Value = client
        .post("https://api.openai.com/v1/chat/completions")
        .bearer_auth(api_key)
        .json(&payload)
        .send()
        .await
        .context("failed to call OpenAI")?
        .error_for_status()
        .context("OpenAI request failed")?
        .json()
        .await
        .context("failed to decode OpenAI response")?;
    body.get("choices")
        .and_then(|v| v.as_array())
        .and_then(|choices| choices.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .map(str::to_string)
        .context("OpenAI response did not contain message content")
}

async fn run_ollama(
    client: &reqwest::Client,
    cfg: &AgentRuntimeConfig,
    system_prompt: &str,
    user_prompt: &str,
) -> Result<String> {
    let model = cfg.model.as_deref().unwrap_or("llama3.1");
    let payload = serde_json::json!({
        "model": model,
        "system": system_prompt,
        "prompt": user_prompt,
        "stream": false
    });
    let body: serde_json::Value = client
        .post(format!("{}/api/generate", cfg.ollama_url))
        .json(&payload)
        .send()
        .await
        .context("failed to call Ollama")?
        .error_for_status()
        .context("Ollama request failed")?
        .json()
        .await
        .context("failed to decode Ollama response")?;
    body.get("response")
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .context("Ollama response did not contain generated text")
}

async fn run_custom(cfg: &AgentRuntimeConfig, user_prompt: &str) -> Result<String> {
    let command = cfg
        .command
        .as_deref()
        .context("backend=custom requires command in ~/.agora/agent.toml")?;
    #[cfg(unix)]
    let mut child = {
        let mut cmd = Command::new("sh");
        cmd.arg("-lc").arg(command);
        cmd
    };
    #[cfg(windows)]
    let mut child = {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    };
    child
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .current_dir(&cfg.project_dir);
    let mut child = child.spawn().context("failed to launch custom backend")?;
    if let Some(stdin) = child.stdin.as_mut() {
        use tokio::io::AsyncWriteExt;
        stdin
            .write_all(user_prompt.as_bytes())
            .await
            .context("failed to write prompt to custom backend")?;
    }
    let output = child
        .wait_with_output()
        .await
        .context("failed to read custom backend output")?;
    if !output.status.success() {
        bail!(
            "custom backend exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn claude_binary() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    let local = PathBuf::from(home)
        .join(".local")
        .join("bin")
        .join("claude");
    if local.exists() {
        local
    } else {
        PathBuf::from("claude")
    }
}

fn codex_binary() -> PathBuf {
    PathBuf::from("codex")
}

fn joined_bodies(messages: &[InboxMessage]) -> String {
    let mut joined = String::new();
    for (idx, message) in messages.iter().enumerate() {
        if idx > 0 {
            joined.push_str("\n\n");
        }
        joined.push_str(&message.body);
    }
    joined
}

fn normalize_reply(reply: &str, send_as: &str) -> Option<String> {
    let reply = reply.trim();
    if reply.is_empty() {
        return None;
    }
    let stripped = reply
        .strip_prefix(&format!("[{}]:", send_as))
        .or_else(|| reply.strip_prefix(&format!("{}:", send_as)))
        .map(str::trim)
        .unwrap_or(reply);
    if stripped.is_empty() {
        None
    } else {
        Some(stripped.to_string())
    }
}

async fn send_direct_reply(
    client: &reqwest::Client,
    api_base: &str,
    to: &str,
    send_as: &str,
    reply_to: Option<&str>,
    conversation_id: Option<&str>,
    body: &str,
) -> Result<()> {
    client
        .post(format!("{}/send", api_base))
        .json(&SendRequest {
            body,
            to: Some(to),
            from: Some(send_as),
            reply_to,
            conversation_id,
        })
        .send()
        .await
        .with_context(|| format!("failed to send direct reply to {}", to))?
        .error_for_status()
        .with_context(|| format!("direct reply failed for {}", to))?;
    Ok(())
}

async fn touch_consumer(client: &reqwest::Client, api_base: &str, consumer_id: u64) -> Result<()> {
    let response = client
        .post(format!("{}/consumers/{}/touch", api_base, consumer_id))
        .send()
        .await
        .with_context(|| format!("failed to touch consumer {}", consumer_id))?;
    if response.status().is_success() || response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(());
    }
    bail!("consumer touch failed with status {}", response.status())
}

async fn run_processing_heartbeat(
    client: reqwest::Client,
    api_base: String,
    consumer_id: u64,
    stop: Arc<Notify>,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(15));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = stop.notified() => break,
            _ = interval.tick() => {
                if let Err(err) = touch_consumer(&client, &api_base, consumer_id).await {
                    warn!("Failed to heartbeat consumer {} during backend processing: {}", consumer_id, err);
                }
            }
        }
    }
}

async fn send_room_reply(
    client: &reqwest::Client,
    api_base: &str,
    route: &RoomRoute,
    send_as: &str,
    body: &str,
) -> Result<()> {
    client
        .post(format!(
            "{}/projects/{}/rooms/{}/send",
            api_base, route.project_id, route.room_id
        ))
        .json(&RoomSendRequest {
            body,
            from: Some(send_as),
        })
        .send()
        .await
        .with_context(|| {
            format!(
                "failed to send room reply to project {} room {}",
                route.project_id, route.room_name
            )
        })?
        .error_for_status()
        .with_context(|| {
            format!(
                "room reply failed for project {} room {}",
                route.project_id, route.room_name
            )
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn inbox(id: &str, from: &str, conversation_id: Option<&str>, body: &str) -> InboxMessage {
        InboxMessage {
            id: id.to_string(),
            from: from.to_string(),
            body: body.to_string(),
            timestamp: "2026-03-19T00:00:00Z".to_string(),
            conversation_id: conversation_id.map(str::to_string),
        }
    }

    #[test]
    fn batches_group_by_sender_and_conversation() {
        let batches = build_batches(vec![
            inbox("1", "alice", Some("thread-1"), "a"),
            inbox("2", "alice", Some("thread-1"), "b"),
            inbox("3", "bob", Some("thread-1"), "c"),
            inbox("4", "alice", Some("thread-2"), "d"),
        ]);

        assert_eq!(batches.len(), 3);
        assert_eq!(batches[0].messages.len(), 2);
        assert_eq!(batches[0].from, "alice");
        assert_eq!(batches[0].conversation_id.as_deref(), Some("thread-1"));
        assert_eq!(batches[1].from, "bob");
        assert_eq!(batches[2].conversation_id.as_deref(), Some("thread-2"));
    }

    #[test]
    fn normalize_reply_strips_agent_prefix() {
        assert_eq!(
            normalize_reply("[codex]: hello", "codex").as_deref(),
            Some("hello")
        );
        assert_eq!(
            normalize_reply("codex: hello", "codex").as_deref(),
            Some("hello")
        );
        assert_eq!(
            normalize_reply("plain reply", "codex").as_deref(),
            Some("plain reply")
        );
    }

    #[test]
    fn self_messages_include_listener_aliases() {
        assert!(is_self_message("codex", "codex", "codex-listener"));
        assert!(is_self_message("codex-listener", "codex", "codex-listener"));
        assert!(is_self_message("codex-helper", "codex", "codex-listener"));
        assert!(!is_self_message("claude", "codex", "codex-listener"));
    }

    #[test]
    fn parse_backend_supports_codex() {
        assert!(matches!(parse_backend("codex"), Ok(Backend::Codex)));
    }

    #[derive(Clone)]
    struct RegisterTestState {
        attempts: Arc<AtomicUsize>,
    }

    async fn flaky_register_handler(
        State(state): State<RegisterTestState>,
    ) -> Result<Json<serde_json::Value>, StatusCode> {
        let attempt = state.attempts.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        Ok(Json(serde_json::json!({ "consumer_id": 42 })))
    }

    #[tokio::test]
    async fn register_consumer_retries_after_transient_failure() {
        let state = RegisterTestState {
            attempts: Arc::new(AtomicUsize::new(0)),
        };
        let app = Router::new()
            .route("/consumers", post(flaky_register_handler))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let consumer_id = register_consumer_with_retry_and_delay(
            &client,
            &format!("http://{}", addr),
            "codex-listener",
            Duration::from_millis(10),
        )
        .await
        .unwrap();

        assert_eq!(consumer_id, 42);
        assert!(state.attempts.load(Ordering::SeqCst) >= 2);

        server.abort();
    }
}
