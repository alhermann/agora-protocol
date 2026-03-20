mod api;
mod auth;
mod child_agent;
mod config;
mod crypto;
mod dashboard;
mod discovery;
mod format;
mod github;
mod identity;
mod mcp;
mod net;
mod outbox;
mod project;
mod protocol;
mod state;
mod thread;

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

use identity::{OwnerAttestation, OwnerIdentity};
use state::{DaemonState, Friend, FriendsStore, TrustLevel};

/// Agora Protocol daemon — secure peer-to-peer AI agent collaboration
#[derive(Parser)]
#[command(name = "agora", version, about, long_about = None)]
struct Cli {
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Node name (identifies this agent on the network)
    #[arg(short, long, global = true, default_value = "agora-node")]
    name: String,

    /// Output format: "table" (default, human-readable) or "json" (machine-readable)
    #[arg(long, global = true, default_value = "table")]
    format: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Agora daemon and listen for connections
    Start {
        /// Address to listen on
        #[arg(short, long, default_value = "0.0.0.0")]
        address: String,

        /// Port to listen on (P2P)
        #[arg(short, long, default_value_t = 7312)]
        port: u16,

        /// Port for the local HTTP API
        #[arg(long, default_value_t = 7313)]
        api_port: u16,

        /// Shell command to run when a message arrives (wake-up hook).
        /// Receives AGORA_FROM and AGORA_PREVIEW env vars.
        #[arg(long)]
        wake_command: Option<String>,

        /// Also connect outbound to a remote peer (host:port).
        /// Can be specified multiple times. All connections share the same
        /// daemon state, inbox, friends, and HTTP API.
        #[arg(long = "connect")]
        connect_targets: Vec<String>,

        /// Auto-connect to friends that have a stored last_address.
        #[arg(long)]
        auto_connect: bool,

        /// Minimum trust level to accept connections (0=anyone, 1-4=require friend).
        #[arg(long, default_value_t = 0)]
        min_trust: u8,

        /// WebSocket relay URL for NAT traversal (e.g., "ws://relay.example.com:8443/ws").
        #[arg(long)]
        relay_url: Option<String>,

        /// Disable data-at-rest encryption (for development).
        #[arg(long)]
        no_encrypt: bool,

        /// Run as a background daemon (detach from terminal, write PID file).
        #[arg(short, long)]
        daemon: bool,
    },

    /// Connect to a remote Agora node
    Connect {
        /// Remote address (host:port)
        target: String,

        /// Port for the local HTTP API
        #[arg(long, default_value_t = 7313)]
        api_port: u16,
    },

    /// Show daemon status
    Status,

    /// Stop a running daemon (reads PID from ~/.agora/agora.pid)
    Stop,

    /// Manage friends
    Friends {
        #[command(subcommand)]
        action: FriendsAction,
    },

    /// Manage projects
    Project {
        #[command(subcommand)]
        action: ProjectAction,
    },

    /// Show connected peers
    Peers,

    /// Read messages from inbox
    Messages {
        /// Long-poll: wait for messages
        #[arg(long)]
        wait: bool,
        /// Timeout in seconds (with --wait)
        #[arg(long, default_value_t = 30)]
        timeout: u64,
    },

    /// Send a message to peers
    Send {
        /// Message body
        body: String,
        /// Send to specific peer (omit for broadcast)
        #[arg(long)]
        to: Option<String>,
    },

    /// Run as an MCP server (stdio transport) for Claude Code integration
    Mcp {
        /// Port of the local HTTP API to bridge to
        #[arg(long, default_value_t = 7313)]
        api_port: u16,

        /// Agent name for this MCP consumer (used as consumer label for multi-agent setups)
        #[arg(long)]
        agent_name: Option<String>,
    },

    /// Run a persistent local child-agent listener against the daemon API
    Agent {
        #[command(subcommand)]
        action: AgentAction,
    },

    /// Manage owner identity (multi-device agent ownership)
    Owner {
        #[command(subcommand)]
        action: OwnerAction,
    },

    /// Manage API authentication token
    Token {
        #[command(subcommand)]
        action: TokenAction,
    },

    /// Generate Claude Code config files (.mcp.json + .claude/settings.local.json)
    /// with all agora MCP tools pre-allowed. Fixes background sub-agent permission blocking.
    SetupClaude {
        /// Agent name for this Claude Code instance
        #[arg(long, default_value = "claude")]
        agent_name: String,

        /// API port the daemon listens on
        #[arg(long, default_value_t = 7313)]
        api_port: u16,

        /// Target directory to write config files (default: current directory)
        #[arg(long)]
        dir: Option<String>,
    },
}

#[derive(Subcommand)]
enum FriendsAction {
    /// List all friends
    List,
    /// Add a friend
    Add {
        /// Name of the agent (must match the node name it connects with)
        name: String,
        /// Trust level 0-4 (0=Unknown, 1=Acquaintance, 2=Friend, 3=Trusted, 4=Inner Circle)
        #[arg(short, long, default_value_t = 2)]
        trust: u8,
        /// Optional alias for this friend
        #[arg(short, long)]
        alias: Option<String>,
        /// Optional notes about this friend
        #[arg(long)]
        notes: Option<String>,
    },
    /// Remove a friend
    Remove {
        /// Name of the agent to remove
        name: String,
    },
    /// List pending friend requests
    Requests,
    /// Accept a friend request by peer name
    Accept {
        /// Name of the peer or request ID
        name: String,
        /// Trust level to assign (0-4)
        #[arg(short, long, default_value_t = 2)]
        trust: u8,
    },
    /// Reject a friend request by peer name
    Reject {
        /// Name of the peer or request ID
        name: String,
    },
}

#[derive(Subcommand)]
enum OwnerAction {
    /// Generate a new owner keypair and attest the current agent
    Init {
        /// Overwrite existing owner key if present
        #[arg(long)]
        force: bool,
    },
    /// Show owner identity and attestation status
    Show,
    /// Export owner key to a file (for transfer to another device)
    Export {
        /// Output file path
        output: String,
    },
    /// Import owner key from a file (and auto-attest current agent)
    Import {
        /// Input file path
        input: String,
    },
}

#[derive(Subcommand)]
enum ProjectAction {
    /// List all projects
    List,
    /// Create a new project
    Create {
        /// Project name
        name: String,
        /// Repository URL
        #[arg(long)]
        repo: Option<String>,
        /// Project description
        #[arg(long)]
        description: Option<String>,
    },
    /// Show project details
    Show {
        /// Project ID (UUID)
        id: String,
    },
    /// Invite a peer to a project
    Invite {
        /// Project ID
        project_id: String,
        /// Peer name
        peer_name: String,
        /// Role (owner, overseer, developer, reviewer, consultant, observer, tester)
        #[arg(long, default_value = "developer")]
        role: String,
        /// Optional message
        #[arg(long)]
        message: Option<String>,
    },
    /// Accept a project invitation
    Join {
        /// Invitation ID (UUID)
        invitation_id: String,
    },
    /// Leave a project
    Leave {
        /// Project ID
        project_id: String,
    },
    /// Clock in to a project
    ClockIn {
        /// Project ID
        project_id: String,
        /// What you're working on
        #[arg(long)]
        focus: Option<String>,
    },
    /// Clock out of a project
    ClockOut {
        /// Project ID
        project_id: String,
    },
    /// List tasks in a project
    Tasks {
        /// Project ID
        project_id: String,
    },
    /// Add a task to a project
    AddTask {
        /// Project ID
        project_id: String,
        /// Task title
        title: String,
        /// Task description
        #[arg(long)]
        description: Option<String>,
        /// Assignee name
        #[arg(long)]
        assignee: Option<String>,
        /// Priority: low, medium, high, critical
        #[arg(long)]
        priority: Option<String>,
    },
    /// Update a task
    UpdateTask {
        /// Project ID
        project_id: String,
        /// Task ID
        task_id: String,
        /// New status: todo, in_progress, done, blocked
        #[arg(long)]
        status: Option<String>,
        /// New assignee
        #[arg(long)]
        assignee: Option<String>,
    },
    /// Get or advance project stage
    Stage {
        /// Project ID
        project_id: String,
        /// Stage name to set: investigation, implementation, review, integration, deployment
        #[arg(long)]
        stage: Option<String>,
        /// Advance to next stage
        #[arg(long)]
        advance: bool,
    },
    /// Show audit trail for a project
    Audit {
        /// Project ID
        project_id: String,
        /// Number of entries to show (0 = all)
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Suspend an agent in a project
    Suspend {
        /// Project ID
        project_id: String,
        /// Agent name to suspend
        agent_name: String,
        /// Reason for suspension
        #[arg(long)]
        reason: Option<String>,
    },
    /// Unsuspend an agent in a project
    Unsuspend {
        /// Project ID
        project_id: String,
        /// Agent name to unsuspend
        agent_name: String,
    },
    /// View project conversation history
    Conversation {
        /// Project ID
        project_id: String,
        /// Max number of messages to show (0 = all)
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Sync project tasks with GitHub issues
    GithubSync {
        /// Project ID
        project_id: String,
    },
    /// Set GitHub personal access token
    GithubToken {
        /// GitHub personal access token (ghp_...)
        token: String,
    },
    /// Show GitHub integration status
    GithubStatus {
        /// Project ID
        project_id: String,
    },
}

#[derive(Subcommand)]
enum TokenAction {
    /// Show the current API token
    Show,
    /// Generate a new API token (invalidates the old one)
    Regenerate,
}

#[derive(Subcommand)]
enum AgentAction {
    /// Listen for inbox messages and answer with the configured agent backend
    Listen {
        /// Port of the local HTTP API to bridge to
        #[arg(long, default_value_t = 7313)]
        api_port: u16,

        /// Registered consumer label for the listener
        #[arg(long)]
        label: Option<String>,

        /// Logical agent name to send messages as (defaults to LABEL-prefix or --name)
        #[arg(long)]
        send_as: Option<String>,

        /// Long-poll timeout in seconds
        #[arg(long, default_value_t = 30)]
        wait_timeout: u64,

        /// Process at most one poll batch, then exit
        #[arg(long)]
        once: bool,

        /// Run the listener in the background
        #[arg(short, long)]
        daemon: bool,
    },
}

fn pid_file_path() -> std::path::PathBuf {
    crate::config::agora_home().join("agora.pid")
}

fn agora_state_dir() -> PathBuf {
    return crate::config::agora_home();
}
fn _agora_state_dir_old() -> PathBuf {
    if let Ok(dir) = std::env::var("AGORA_HOME") {
        return PathBuf::from(dir);
    }
    crate::config::agora_home()
}

fn sanitized_listener_label(label: &str) -> String {
    label
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => ch,
            _ => '_',
        })
        .collect()
}

fn listener_log_path(label: &str) -> PathBuf {
    agora_state_dir()
        .join("listeners")
        .join(format!("{}.log", sanitized_listener_label(label)))
}

fn default_listener_send_as(cli_name: &str, label: Option<&str>) -> String {
    label
        .and_then(|value| value.strip_suffix("-listener"))
        .filter(|value| !value.is_empty())
        .unwrap_or(cli_name)
        .to_string()
}

fn spawn_detached_reexec(args: &[String], log_path: &Path) -> anyhow::Result<std::process::Child> {
    let exe = std::env::current_exe()?;
    if let Some(parent) = log_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let stdout = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    let stderr = stdout.try_clone()?;

    let mut cmd = std::process::Command::new(exe);
    cmd.args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(stdout))
        .stderr(std::process::Stdio::from(stderr));

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;

        // Detach from the caller's controlling terminal so the listener
        // survives Codex/TTY session teardown.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }

    Ok(cmd.spawn()?)
}

fn write_pid_file() {
    let path = pid_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, std::process::id().to_string());
}

fn remove_pid_file() {
    let _ = std::fs::remove_file(pid_file_path());
}

fn setup_logging(verbose: bool, stderr_only: bool) {
    let filter = if verbose {
        EnvFilter::new("agora=debug,info")
    } else {
        EnvFilter::new("agora=info,warn")
    };

    if stderr_only {
        // MCP mode: stdout is the transport channel, all logs go to stderr
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .init();
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Install default crypto provider for rustls
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    let cli = Cli::parse();
    let mcp_mode = matches!(cli.command, Commands::Mcp { .. });
    setup_logging(cli.verbose, mcp_mode);

    // Load config file (CLI flags override config values)
    let cfg = config::AgoraConfig::load(&config::AgoraConfig::default_path());

    match cli.command {
        Commands::Start {
            address,
            port,
            api_port,
            wake_command,
            connect_targets,
            auto_connect,
            min_trust,
            relay_url,
            no_encrypt,
            daemon,
        } => {
            // Merge config: CLI flags take precedence over config file
            let cfg_addrs = cfg.connect_addresses();
            let address = if address != "0.0.0.0" {
                address
            } else {
                cfg.address.unwrap_or(address)
            };
            let port = if port != 7312 {
                port
            } else {
                cfg.p2p_port.unwrap_or(port)
            };
            let api_port = if api_port != 7313 {
                api_port
            } else {
                cfg.api_port.unwrap_or(api_port)
            };
            let wake_command = wake_command.or(cfg.wake_command);
            let auto_connect = auto_connect || cfg.auto_connect.unwrap_or(false);
            let min_trust = if min_trust > 0 {
                min_trust
            } else {
                cfg.min_trust.unwrap_or(0)
            };
            let auto_accept_policy = config::AutoAcceptPolicy::from_str(
                cfg.auto_accept.as_deref().unwrap_or("same_owner"),
            );
            let relay_url = relay_url.or(cfg.relay_url);
            let mut connect_targets = connect_targets;
            for addr in cfg_addrs {
                if !connect_targets.contains(&addr) {
                    connect_targets.push(addr);
                }
            }
            if daemon {
                // Re-exec ourselves in the background
                let exe = std::env::current_exe()?;
                let mut args: Vec<String> = std::env::args().collect();
                // Remove --daemon / -d flag to prevent infinite recursion
                args.retain(|a| a != "--daemon" && a != "-d");
                let child = std::process::Command::new(exe)
                    .args(&args[1..])
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()?;
                println!("Agora daemon started in background (PID: {})", child.id());
                println!("Stop with: agora stop");
                return Ok(());
            }

            // Write PID file for `agora stop`
            write_pid_file();

            let friends_path = FriendsStore::default_path();
            let node_name = if cli.name != "agora-node" {
                cli.name.clone()
            } else {
                cfg.name.unwrap_or(cli.name.clone())
            };
            let state = DaemonState::new(&node_name, &friends_path, api_port);

            state.set_auto_accept_policy(auto_accept_policy);
            info!("Auto-accept policy: {:?}", auto_accept_policy);

            if min_trust > 0 {
                state.set_min_trust(min_trust.min(4));
                info!("Connection policy: min_trust = {}", min_trust);
            }

            if let Some(ref cmd) = wake_command {
                match state.set_wake_command(Some(cmd.clone())).await {
                    Ok(()) => info!("Wake-up hook set: {}", cmd),
                    Err(e) => {
                        eprintln!("Invalid wake command: {}", e);
                        std::process::exit(1);
                    }
                }
            }

            // Auto-configure wake hook if none exists (from CLI flag or persisted file)
            if state.get_wake_command().await.is_none() {
                let default_script = "./daemon/wake-agent.sh".to_string();
                if std::path::Path::new(&default_script).exists() {
                    info!("Auto-configuring wake hook: {}", default_script);
                    let _ = state.set_wake_command(Some(default_script)).await;
                }
            }

            // Kill orphaned MCP bridge processes older than 10 seconds
            // (avoids killing bridges that just started for active agents)
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("bash")
                    .args(["-c", "pgrep -f 'agora.*mcp.*--api-port' | while read pid; do age=$(ps -o etime= -p $pid 2>/dev/null | tr -d ' '); if [ -n \"$age\" ] && echo $age | grep -q ':'; then kill $pid 2>/dev/null; fi; done"])
                    .status();
                info!("Cleaned up orphaned MCP bridge processes (older than 10s only)");
            }

            println!("Agora daemon starting...");
            println!("  Node name:  {}", node_name);
            println!("  P2P listen: {}:{}", address, port);
            println!("  HTTP API:   127.0.0.1:{}", api_port);
            if let Some(ref cmd) = wake_command {
                println!("  Wake hook:  {}", cmd);
            }
            if min_trust > 0 {
                println!("  Min trust:  {} (reject unknown peers)", min_trust);
            }
            for target in &connect_targets {
                println!("  Connect to: {}", target);
            }
            if let Some(ref url) = relay_url {
                println!("  Relay:      {}", url);
            }
            println!();

            // Run network listener and HTTP API concurrently
            let net_state = state.clone();
            let net_address = address.clone();
            let net_task =
                tokio::spawn(
                    async move { net::start_listener(net_state, &net_address, port).await },
                );

            let api_state = state.clone();
            let api_task = tokio::spawn(async move {
                let app = api::router(api_state, true);
                let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", api_port))
                    .await
                    .expect("Failed to bind HTTP API");
                info!("HTTP API listening on 127.0.0.1:{}", api_port);
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
                )
                .await
                .expect("HTTP API server failed");
            });

            // Spawn outbound connections — same DaemonState, shared inbox/friends/outbox
            let mut connect_set: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            for target in connect_targets {
                connect_set.insert(target.clone());
                let conn_state = state.clone();
                tokio::spawn(async move { net::connect_to_peer(conn_state, &target).await });
            }

            // Auto-connect to friends with stored addresses
            if auto_connect {
                let auto_friends = state.friends_with_addresses().await;
                for (friend, addr) in auto_friends {
                    if connect_set.contains(&addr) {
                        continue; // Already connecting explicitly
                    }
                    info!("Auto-connecting to friend {} at {}", friend.name, addr);
                    println!("  Auto-connect: {} ({})", friend.name, addr);
                    let conn_state = state.clone();
                    tokio::spawn(async move { net::connect_to_peer(conn_state, &addr).await });
                }
            }

            // Connect to relay if configured
            if let Some(ref url) = relay_url {
                let relay_state = state.clone();
                let relay_url = url.clone();
                tokio::spawn(async move {
                    if let Err(e) = net::connect_to_relay(relay_state, &relay_url).await {
                        error!("Relay connection error: {}", e);
                    }
                });
            }

            // Restore conversation history from previous session
            state.load_conversation_history().await;

            // --- Periodic GitHub sync (every 5 minutes) ---
            {
                let sync_state = state.clone();
                tokio::spawn(async move {
                    // Wait 30s after startup before first sync
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    loop {
                        // Sync all active projects that have a GitHub repo
                        let cfg = crate::github::GitHubConfig::load();
                        if let Some(ref token) = cfg.token {
                            let projects = sync_state.get_projects().await;
                            for p in projects {
                                if p.status != crate::project::ProjectStatus::Active {
                                    continue;
                                }
                                let repo_url = match sync_state.get_project_repo(&p.id).await {
                                    Some(Some(url)) => url,
                                    _ => continue,
                                };
                                let (owner, repo) =
                                    match crate::github::parse_github_repo(&repo_url) {
                                        Some(pair) => pair,
                                        None => continue,
                                    };
                                let existing_tasks = match sync_state.get_project_tasks(&p.id).await
                                {
                                    Some(tasks) => tasks,
                                    None => continue,
                                };
                                match crate::github::sync_bidirectional(
                                    token,
                                    &owner,
                                    &repo,
                                    &existing_tasks,
                                )
                                .await
                                {
                                    Ok(result) => {
                                        if result.imported > 0 || result.pushed > 0 {
                                            info!(
                                                "GitHub auto-sync for {}: imported={}, pushed={}, errors={}",
                                                p.name,
                                                result.imported,
                                                result.pushed,
                                                result.errors.len()
                                            );
                                            // Import new tasks
                                            if result.imported > 0 {
                                                if let Ok(remote_tasks) =
                                                    crate::github::import_issues(
                                                        token, &owner, &repo,
                                                    )
                                                    .await
                                                {
                                                    let existing_issue_nums: std::collections::HashSet<u64> = existing_tasks
                                                        .iter()
                                                        .filter_map(|t| t.github_issue_number)
                                                        .collect();
                                                    for task in remote_tasks {
                                                        if let Some(num) = task.github_issue_number
                                                        {
                                                            if !existing_issue_nums.contains(&num) {
                                                                sync_state
                                                                    .create_task_with_id(
                                                                        &p.id,
                                                                        Some(task.id),
                                                                        &task.title,
                                                                        task.description.clone(),
                                                                        task.assignee.clone(),
                                                                        task.priority.clone(),
                                                                        task.depends_on.clone(),
                                                                        Some(
                                                                            "github-sync"
                                                                                .to_string(),
                                                                        ),
                                                                    )
                                                                    .await;
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("GitHub auto-sync error for {}: {}", p.name, e);
                                    }
                                }
                            }
                        }
                        // Sync every 5 minutes
                        tokio::time::sleep(Duration::from_secs(300)).await;
                    }
                });
            }

            // Set up signal handlers for graceful shutdown (SIGINT + SIGTERM)
            let shutdown_state = state.clone();

            // Create a future that completes on SIGTERM (unix) or never (non-unix)
            #[cfg(unix)]
            let sigterm_fut = async {
                let mut sigterm =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("Failed to install SIGTERM handler");
                sigterm.recv().await;
            };
            #[cfg(not(unix))]
            let sigterm_fut = std::future::pending::<()>();

            tokio::select! {
                result = net_task => {
                    result??;
                },
                result = api_task => {
                    result?;
                },
                _ = tokio::signal::ctrl_c() => {
                    println!("\nReceived SIGINT (Ctrl+C), shutting down gracefully...");
                    shutdown_state.graceful_shutdown().await;
                    remove_pid_file();
                },
                _ = sigterm_fut => {
                    println!("\nReceived SIGTERM, shutting down gracefully...");
                    shutdown_state.graceful_shutdown().await;
                    remove_pid_file();
                },
            }
        }

        Commands::Connect { target, api_port } => {
            let friends_path = FriendsStore::default_path();
            let state = DaemonState::new(&cli.name, &friends_path, api_port);

            println!("Connecting to {}...", target);
            println!("  HTTP API: 127.0.0.1:{}", api_port);
            println!();

            // Run connector and HTTP API concurrently
            let net_state = state.clone();
            let net_task =
                tokio::spawn(async move { net::connect_to_peer(net_state, &target).await });

            let api_state = state.clone();
            let api_task = tokio::spawn(async move {
                let app = api::router(api_state, true);
                let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", api_port))
                    .await
                    .expect("Failed to bind HTTP API");
                info!("HTTP API listening on 127.0.0.1:{}", api_port);
                axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
                )
                .await
                .expect("HTTP API server failed");
            });

            let shutdown_state = state.clone();

            #[cfg(unix)]
            let sigterm_connect_fut = async {
                let mut sigterm =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                        .expect("Failed to install SIGTERM handler");
                sigterm.recv().await;
            };
            #[cfg(not(unix))]
            let sigterm_connect_fut = std::future::pending::<()>();

            tokio::select! {
                result = net_task => { result??; },
                result = api_task => { result?; },
                _ = tokio::signal::ctrl_c() => {
                    println!("\nReceived SIGINT (Ctrl+C), shutting down gracefully...");
                    shutdown_state.graceful_shutdown().await;
                },
                _ = sigterm_connect_fut => {
                    println!("\nReceived SIGTERM, shutting down gracefully...");
                    shutdown_state.graceful_shutdown().await;
                },
            }
            remove_pid_file();
        }

        Commands::Status => {
            let api_base = "http://127.0.0.1:7313";
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap();

            match client.get(format!("{}/status", api_base)).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let data: serde_json::Value = resp.json().await.unwrap_or_default();
                    if cli.format == "json" {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&data).unwrap_or_default()
                        );
                    } else {
                        let name = data
                            .get("node_name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("?");
                        let version = data.get("version").and_then(|v| v.as_str()).unwrap_or("?");
                        let did = data.get("did").and_then(|v| v.as_str()).unwrap_or("?");
                        let peers = data
                            .get("peers_connected")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let owner_did = data.get("owner_did").and_then(|v| v.as_str());

                        println!();
                        println!("  {}", format::bold("Agora Daemon Status"));
                        println!();
                        format::print_kv("Node name", name);
                        format::print_kv("Version", version);
                        format::print_kv("DID", &format::dim(&format::short_id(did)));
                        format::print_kv("Peers", &peers.to_string());
                        if let Some(od) = owner_did {
                            format::print_kv("Owner DID", &format::dim(&format::short_id(od)));
                        }
                        println!();
                    }
                }
                _ => {
                    println!("  {} Daemon not running", format::red("●"));
                    println!("  Version: {}", env!("CARGO_PKG_VERSION"));
                    let pid_path = pid_file_path();
                    if pid_path.exists() {
                        if let Ok(pid_str) = std::fs::read_to_string(&pid_path) {
                            println!(
                                "  PID file: {} (PID: {})",
                                pid_path.display(),
                                pid_str.trim()
                            );
                        }
                    }
                }
            }
        }

        Commands::Peers => {
            let api_base = "http://127.0.0.1:7313";
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap();
            match client.get(format!("{}/peers", api_base)).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let data: serde_json::Value = resp.json().await.unwrap_or_default();
                    if cli.format == "json" {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&data).unwrap_or_default()
                        );
                    } else {
                        let peers = data
                            .get("peers")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();
                        if peers.is_empty() {
                            println!("  No peers connected.");
                        } else {
                            println!();
                            let headers = vec!["Name", "Address", "Verified", "Connected"];
                            let rows: Vec<Vec<String>> = peers
                                .iter()
                                .map(|p| {
                                    vec![
                                        p.get("name")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("?")
                                            .to_string(),
                                        p.get("address")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("?")
                                            .to_string(),
                                        if p.get("verified")
                                            .and_then(|v| v.as_bool())
                                            .unwrap_or(false)
                                        {
                                            format::green("yes")
                                        } else {
                                            format::dim("no")
                                        },
                                        p.get("connected_at")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("?")
                                            .chars()
                                            .take(19)
                                            .collect(),
                                    ]
                                })
                                .collect();
                            println!("  Connected peers ({}):", peers.len());
                            println!();
                            format::print_table(&headers, &rows);
                            println!();
                        }
                    }
                }
                _ => {
                    eprintln!("Daemon not running. Start with: agora start");
                    std::process::exit(1);
                }
            }
        }

        Commands::Messages { wait, timeout } => {
            let api_base = "http://127.0.0.1:7313";
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(timeout + 5))
                .build()
                .unwrap();
            let url = if wait {
                format!("{}/messages?wait=true&timeout={}", api_base, timeout)
            } else {
                format!("{}/messages", api_base)
            };
            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    let data: serde_json::Value = resp.json().await.unwrap_or_default();
                    if cli.format == "json" {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&data).unwrap_or_default()
                        );
                    } else {
                        let msgs = data.as_array().cloned().unwrap_or_default();
                        if msgs.is_empty() {
                            println!("  No messages.");
                        } else {
                            for m in &msgs {
                                let from = m.get("from").and_then(|v| v.as_str()).unwrap_or("?");
                                let body = m.get("body").and_then(|v| v.as_str()).unwrap_or("");
                                let ts = m
                                    .get("timestamp")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .chars()
                                    .take(19)
                                    .collect::<String>();
                                println!(
                                    "  {} [{}] {}",
                                    format::bold(from),
                                    format::dim(&ts),
                                    body
                                );
                            }
                        }
                    }
                }
                _ => {
                    eprintln!("Daemon not running.");
                    std::process::exit(1);
                }
            }
        }

        Commands::Send { body, to } => {
            let api_base = "http://127.0.0.1:7313";
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap();
            let payload = serde_json::json!({ "body": body, "to": to });
            match client
                .post(format!("{}/send", api_base))
                .json(&payload)
                .send()
                .await
            {
                Ok(resp) if resp.status().is_success() => {
                    let data: serde_json::Value = resp.json().await.unwrap_or_default();
                    if cli.format == "json" {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&data).unwrap_or_default()
                        );
                    } else {
                        let id = data.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                        let target = to.as_deref().unwrap_or("all peers");
                        println!(
                            "  {} Message sent to {} ({})",
                            format::green("✓"),
                            target,
                            format::short_id(id)
                        );
                    }
                }
                _ => {
                    eprintln!("Failed to send message. Is daemon running?");
                    std::process::exit(1);
                }
            }
        }

        Commands::Stop => {
            let pid_path = pid_file_path();
            match std::fs::read_to_string(&pid_path) {
                Ok(pid_str) => {
                    let pid_str = pid_str.trim();
                    match pid_str.parse::<u32>() {
                        Ok(pid) => {
                            // Send SIGTERM for graceful shutdown
                            let status = std::process::Command::new("kill")
                                .arg(pid.to_string())
                                .status();
                            match status {
                                Ok(s) if s.success() => {
                                    println!(
                                        "Sent SIGTERM to Agora daemon (PID: {}), waiting for exit...",
                                        pid
                                    );
                                    // Wait for the process to actually exit (up to 10 seconds)
                                    let wait_start = std::time::Instant::now();
                                    let wait_timeout = std::time::Duration::from_secs(10);
                                    loop {
                                        // Check if process is still alive with kill -0
                                        let alive = std::process::Command::new("kill")
                                            .args(["-0", &pid.to_string()])
                                            .stdout(std::process::Stdio::null())
                                            .stderr(std::process::Stdio::null())
                                            .status()
                                            .map(|s| s.success())
                                            .unwrap_or(false);
                                        if !alive {
                                            break;
                                        }
                                        if wait_start.elapsed() >= wait_timeout {
                                            eprintln!(
                                                "Warning: daemon (PID: {}) still running after {:?}. \
                                                 It may still be saving state. Use 'kill -9 {}' to force.",
                                                pid, wait_timeout, pid
                                            );
                                            break;
                                        }
                                        std::thread::sleep(std::time::Duration::from_millis(200));
                                    }
                                    let _ = std::fs::remove_file(&pid_path);
                                    println!("Stopped Agora daemon (PID: {})", pid);
                                }
                                _ => {
                                    eprintln!(
                                        "Failed to stop daemon (PID: {}). Process may not be running.",
                                        pid
                                    );
                                    let _ = std::fs::remove_file(&pid_path);
                                }
                            }
                        }
                        Err(_) => {
                            eprintln!("Invalid PID in {}", pid_path.display());
                        }
                    }
                }
                Err(_) => {
                    eprintln!(
                        "No PID file found at {}. Is the daemon running?",
                        pid_path.display()
                    );
                }
            }
        }

        Commands::Friends { action } => {
            // Try to reach the running daemon's API first (default port 7313).
            // If the daemon is running, proxy through the API so in-memory state
            // stays in sync. Fall back to direct file editing if daemon is offline.
            let api_base = "http://127.0.0.1:7313";
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(2))
                .build()
                .unwrap();
            let daemon_running = client
                .get(format!("{}/status", api_base))
                .send()
                .await
                .is_ok();

            match action {
                FriendsAction::List => {
                    let print_friends_table = |friends: &[Friend]| {
                        if cli.format == "json" {
                            println!(
                                "{}",
                                serde_json::to_string_pretty(&friends).unwrap_or_default()
                            );
                            return;
                        }
                        if friends.is_empty() {
                            println!("  No friends yet. Add one with: agora friends add <name>");
                            return;
                        }
                        println!();
                        println!("  Friends ({}):", friends.len());
                        println!();
                        let headers = vec!["Name", "Trust", "Status", "Added"];
                        let rows: Vec<Vec<String>> = friends
                            .iter()
                            .map(|f| {
                                let trust_str = match f.trust_level.0 {
                                    0 => format::dim("Unknown"),
                                    1 => "Acquaintance".to_string(),
                                    2 => format::green("Friend"),
                                    3 => format::yellow("Trusted"),
                                    4 => format::cyan("Inner Circle"),
                                    _ => "?".to_string(),
                                };
                                let alias_part = f
                                    .alias
                                    .as_deref()
                                    .map(|a| format!(" ({})", a))
                                    .unwrap_or_default();
                                vec![
                                    format!("{}{}", f.name, alias_part),
                                    trust_str,
                                    if f.muted {
                                        format::dim("muted")
                                    } else {
                                        String::new()
                                    },
                                    f.added_at.format("%Y-%m-%d").to_string(),
                                ]
                            })
                            .collect();
                        format::print_table(&headers, &rows);
                        println!();
                    };

                    if daemon_running {
                        match client.get(format!("{}/friends", api_base)).send().await {
                            Ok(resp) if resp.status().is_success() => {
                                let data: serde_json::Value = resp.json().await.unwrap_or_default();
                                let friends: Vec<Friend> = data
                                    .get("friends")
                                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                                    .unwrap_or_default();
                                print_friends_table(&friends);
                            }
                            _ => {
                                eprintln!("Warning: daemon API unreachable, reading from file");
                                let path = FriendsStore::default_path();
                                let store = FriendsStore::load(&path);
                                let friends: Vec<Friend> =
                                    store.list().into_iter().cloned().collect();
                                print_friends_table(&friends);
                            }
                        }
                    } else {
                        let path = FriendsStore::default_path();
                        let store = FriendsStore::load(&path);
                        let friends: Vec<Friend> = store.list().into_iter().cloned().collect();
                        print_friends_table(&friends);
                    }
                }
                FriendsAction::Add {
                    name,
                    trust,
                    alias,
                    notes,
                } => {
                    let trust_level = TrustLevel(trust.min(4));
                    if daemon_running {
                        let body = serde_json::json!({
                            "name": name,
                            "trust_level": trust_level.0,
                            "alias": alias,
                            "notes": notes,
                        });
                        match client
                            .post(format!("{}/friends", api_base))
                            .json(&body)
                            .send()
                            .await
                        {
                            Ok(resp) if resp.status().is_success() => {
                                let data: serde_json::Value = resp.json().await.unwrap_or_default();
                                if let Some(warning) = data.get("warning").and_then(|w| w.as_str())
                                {
                                    eprintln!("Warning: {}", warning);
                                }
                                println!("Added friend: {} (trust {})", name, trust_level);
                                if trust_level.can_wake() {
                                    println!("  This friend CAN trigger your wake-up hook.");
                                } else {
                                    println!(
                                        "  This friend cannot trigger your wake-up hook (need trust >= 3)."
                                    );
                                }
                            }
                            Ok(resp) => {
                                eprintln!("API error: {}", resp.status());
                                std::process::exit(1);
                            }
                            Err(e) => {
                                eprintln!("API request failed: {}", e);
                                std::process::exit(1);
                            }
                        }
                    } else {
                        // Daemon offline — write directly to file
                        let path = FriendsStore::default_path();
                        let mut store = FriendsStore::load(&path);
                        let friend = Friend {
                            name: name.clone(),
                            alias,
                            trust_level,
                            added_at: chrono::Utc::now(),
                            notes,
                            muted: false,
                            last_address: None,
                            did: None,
                            owner_did: None,
                            their_trust: None,
                        };
                        if let Some(warning) = store.add(friend) {
                            eprintln!("Warning: {}", warning);
                        }
                        if let Err(e) = store.save() {
                            eprintln!("Failed to save friends: {}", e);
                            std::process::exit(1);
                        }
                        println!(
                            "Added friend: {} (trust {}) [offline — file only]",
                            name, trust_level
                        );
                        if trust_level.can_wake() {
                            println!("  This friend CAN trigger your wake-up hook.");
                        } else {
                            println!(
                                "  This friend cannot trigger your wake-up hook (need trust >= 3)."
                            );
                        }
                    }
                }
                FriendsAction::Remove { name } => {
                    if daemon_running {
                        match client
                            .delete(format!("{}/friends/{}", api_base, name))
                            .send()
                            .await
                        {
                            Ok(resp) if resp.status().is_success() => {
                                println!("Removed friend: {}", name);
                            }
                            Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => {
                                println!("Friend not found: {}", name);
                            }
                            Ok(resp) => {
                                eprintln!("API error: {}", resp.status());
                                std::process::exit(1);
                            }
                            Err(e) => {
                                eprintln!("API request failed: {}", e);
                                std::process::exit(1);
                            }
                        }
                    } else {
                        let path = FriendsStore::default_path();
                        let mut store = FriendsStore::load(&path);
                        if store.remove(&name) {
                            if let Err(e) = store.save() {
                                eprintln!("Failed to save friends: {}", e);
                                std::process::exit(1);
                            }
                            println!("Removed friend: {} [offline — file only]", name);
                        } else {
                            println!("Friend not found: {}", name);
                        }
                    }
                }
                FriendsAction::Requests => {
                    if !daemon_running {
                        // Read directly from file when offline
                        let path = state::FriendRequestStore::default_path();
                        let store = state::FriendRequestStore::load(&path);
                        let pending: Vec<_> = store
                            .pending_inbound()
                            .into_iter()
                            .chain(store.pending_outbound().into_iter())
                            .collect();
                        if pending.is_empty() {
                            println!("No pending friend requests.");
                        } else {
                            println!("Pending friend requests ({}):", pending.len());
                            for r in pending {
                                let dir = if r.direction == state::FriendRequestDirection::Inbound {
                                    "inbound"
                                } else {
                                    "outbound"
                                };
                                println!(
                                    "  {} ({}) — trust {} [{}]",
                                    r.peer_name, dir, r.offered_trust, r.id
                                );
                                if let Some(ref msg) = r.message {
                                    println!("    message: {}", msg);
                                }
                            }
                        }
                        return Ok(());
                    }
                    match client
                        .get(format!("{}/friend-requests?status=pending", api_base))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            let data: serde_json::Value = resp.json().await.unwrap_or_default();
                            let requests = data
                                .get("requests")
                                .and_then(|v| v.as_array())
                                .cloned()
                                .unwrap_or_default();
                            if requests.is_empty() {
                                println!("No pending friend requests.");
                            } else {
                                println!("Pending friend requests ({}):", requests.len());
                                for r in &requests {
                                    let name =
                                        r.get("peer_name").and_then(|v| v.as_str()).unwrap_or("?");
                                    let dir =
                                        r.get("direction").and_then(|v| v.as_str()).unwrap_or("?");
                                    let trust = r
                                        .get("offered_trust")
                                        .and_then(|v| v.as_u64())
                                        .unwrap_or(0);
                                    let id = r.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                                    let msg = r.get("message").and_then(|v| v.as_str());
                                    println!("  {} ({}) — trust {} [{}]", name, dir, trust, id);
                                    if let Some(msg) = msg {
                                        println!("    message: {}", msg);
                                    }
                                }
                            }
                        }
                        _ => {
                            eprintln!("Failed to fetch friend requests from daemon API.");
                            std::process::exit(1);
                        }
                    }
                }
                FriendsAction::Accept { name, trust } => {
                    if !daemon_running {
                        eprintln!("Daemon must be running to accept friend requests.");
                        std::process::exit(1);
                    }
                    // First look up the request by peer name
                    let requests_resp = client
                        .get(format!("{}/friend-requests?status=pending", api_base))
                        .send()
                        .await;
                    let request_id = match requests_resp {
                        Ok(resp) if resp.status().is_success() => {
                            let data: serde_json::Value = resp.json().await.unwrap_or_default();
                            data.get("requests")
                                .and_then(|v| v.as_array())
                                .and_then(|arr| {
                                    arr.iter().find(|r| {
                                        r.get("peer_name").and_then(|v| v.as_str()) == Some(&name)
                                            && r.get("direction").and_then(|v| v.as_str())
                                                == Some("inbound")
                                    })
                                })
                                .and_then(|r| {
                                    r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string())
                                })
                                // Also try treating the name as a request ID directly
                                .or_else(|| uuid::Uuid::parse_str(&name).ok().map(|_| name.clone()))
                        }
                        _ => None,
                    };
                    match request_id {
                        Some(id) => {
                            let body = serde_json::json!({ "trust_level": trust.min(4) });
                            match client
                                .post(format!("{}/friend-requests/{}/accept", api_base, id))
                                .json(&body)
                                .send()
                                .await
                            {
                                Ok(resp) if resp.status().is_success() => {
                                    println!(
                                        "Accepted friend request from {} (trust {})",
                                        name,
                                        TrustLevel(trust.min(4))
                                    );
                                }
                                Ok(resp) => {
                                    eprintln!("API error: {}", resp.status());
                                    std::process::exit(1);
                                }
                                Err(e) => {
                                    eprintln!("API request failed: {}", e);
                                    std::process::exit(1);
                                }
                            }
                        }
                        None => {
                            eprintln!("No pending inbound friend request found for: {}", name);
                            std::process::exit(1);
                        }
                    }
                }
                FriendsAction::Reject { name } => {
                    if !daemon_running {
                        eprintln!("Daemon must be running to reject friend requests.");
                        std::process::exit(1);
                    }
                    // Look up request by peer name
                    let requests_resp = client
                        .get(format!("{}/friend-requests?status=pending", api_base))
                        .send()
                        .await;
                    let request_id = match requests_resp {
                        Ok(resp) if resp.status().is_success() => {
                            let data: serde_json::Value = resp.json().await.unwrap_or_default();
                            data.get("requests")
                                .and_then(|v| v.as_array())
                                .and_then(|arr| {
                                    arr.iter().find(|r| {
                                        r.get("peer_name").and_then(|v| v.as_str()) == Some(&name)
                                            && r.get("direction").and_then(|v| v.as_str())
                                                == Some("inbound")
                                    })
                                })
                                .and_then(|r| {
                                    r.get("id").and_then(|v| v.as_str()).map(|s| s.to_string())
                                })
                                .or_else(|| uuid::Uuid::parse_str(&name).ok().map(|_| name.clone()))
                        }
                        _ => None,
                    };
                    match request_id {
                        Some(id) => {
                            let body = serde_json::json!({});
                            match client
                                .post(format!("{}/friend-requests/{}/reject", api_base, id))
                                .json(&body)
                                .send()
                                .await
                            {
                                Ok(resp) if resp.status().is_success() => {
                                    println!("Rejected friend request from {}", name);
                                }
                                Ok(resp) => {
                                    eprintln!("API error: {}", resp.status());
                                    std::process::exit(1);
                                }
                                Err(e) => {
                                    eprintln!("API request failed: {}", e);
                                    std::process::exit(1);
                                }
                            }
                        }
                        None => {
                            eprintln!("No pending inbound friend request found for: {}", name);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }

        Commands::Mcp {
            api_port,
            agent_name,
        } => {
            let server = mcp::AgoraMcpServer::new(api_port, agent_name);
            server.run().await?;
        }

        Commands::Agent { action } => match action {
            AgentAction::Listen {
                api_port,
                label,
                send_as,
                wait_timeout,
                once,
                daemon,
            } => {
                let send_as = send_as
                    .unwrap_or_else(|| default_listener_send_as(&cli.name, label.as_deref()));
                let listener_label = label.unwrap_or_else(|| format!("{}-listener", send_as));

                if daemon {
                    let mut args: Vec<String> = std::env::args().collect();
                    args.retain(|a| a != "--daemon" && a != "-d");
                    let log_path = listener_log_path(&listener_label);
                    let child = spawn_detached_reexec(&args[1..], &log_path)?;
                    println!(
                        "Agora child-agent started in background (PID: {}, log: {})",
                        child.id(),
                        log_path.display()
                    );
                    return Ok(());
                }

                child_agent::listen(child_agent::ListenOptions {
                    api_port,
                    listener_label,
                    send_as,
                    wait_timeout_secs: wait_timeout,
                    once,
                })
                .await?;
            }
        },

        Commands::Owner { action } => {
            let owner_path = OwnerIdentity::default_path();
            let att_path = OwnerAttestation::default_path();
            let identity_path = identity::AgentIdentity::default_path();

            match action {
                OwnerAction::Init { force } => {
                    if owner_path.exists() && !force {
                        eprintln!(
                            "Owner key already exists at {}. Use --force to overwrite.",
                            owner_path.display()
                        );
                        std::process::exit(1);
                    }

                    let owner = OwnerIdentity::generate()?;
                    owner.save(&owner_path)?;
                    println!("Generated owner identity:");
                    println!("  DID:  {}", owner.did());
                    println!("  Key:  {}", owner_path.display());

                    // Auto-attest the current agent
                    let agent = identity::AgentIdentity::load_or_create(&identity_path)?;
                    let att = owner.attest_agent(agent.did());
                    att.save(&att_path)?;
                    println!("  Attested agent: {}", agent.did());
                    println!("  Attestation:    {}", att_path.display());
                }

                OwnerAction::Show => {
                    if !owner_path.exists() {
                        println!("No owner identity configured.");
                        println!("Run `agora owner init` to create one.");
                        return Ok(());
                    }

                    let owner = OwnerIdentity::load(&owner_path)?;
                    println!("Owner Identity:");
                    println!("  DID:        {}", owner.did());
                    println!("  Public Key: {}", owner.public_key_base58());
                    println!("  Key File:   {}", owner_path.display());

                    if att_path.exists() {
                        match OwnerAttestation::load(&att_path) {
                            Ok(att) => {
                                let agent =
                                    identity::AgentIdentity::load_or_create(&identity_path)?;
                                let valid = att.verify_for_agent(agent.did());
                                println!();
                                println!("Owner Attestation:");
                                println!("  Agent DID:  {}", att.agent_did);
                                println!("  Owner DID:  {}", att.owner_did);
                                println!(
                                    "  Created:    {}",
                                    chrono::DateTime::from_timestamp(att.created_at, 0)
                                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
                                        .unwrap_or_else(|| att.created_at.to_string())
                                );
                                println!(
                                    "  Valid:      {}",
                                    if valid {
                                        "YES"
                                    } else {
                                        "NO (agent DID mismatch or bad signature)"
                                    }
                                );
                            }
                            Err(e) => {
                                println!("  Attestation: FAILED to load ({})", e);
                            }
                        }
                    } else {
                        println!("  Attestation: none");
                    }
                }

                OwnerAction::Export { output } => {
                    if !owner_path.exists() {
                        eprintln!("No owner key to export. Run `agora owner init` first.");
                        std::process::exit(1);
                    }
                    let owner = OwnerIdentity::load(&owner_path)?;
                    let output_path = std::path::Path::new(&output);
                    std::fs::write(output_path, owner.pkcs8_bytes())?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        let perms = std::fs::Permissions::from_mode(0o600);
                        std::fs::set_permissions(output_path, perms)?;
                    }
                    println!("Exported owner key to: {}", output);
                    println!("  DID: {}", owner.did());
                    println!("  Transfer this file securely to your other device,");
                    println!("  then run: agora owner import {}", output);
                }

                OwnerAction::Import { input } => {
                    let input_path = std::path::Path::new(&input);
                    if !input_path.exists() {
                        eprintln!("File not found: {}", input);
                        std::process::exit(1);
                    }
                    let bytes = std::fs::read(input_path)?;
                    let owner = OwnerIdentity::from_pkcs8_bytes(&bytes)?;
                    owner.save(&owner_path)?;
                    println!("Imported owner identity:");
                    println!("  DID:  {}", owner.did());
                    println!("  Key:  {}", owner_path.display());

                    // Auto-attest the current agent
                    let agent = identity::AgentIdentity::load_or_create(&identity_path)?;
                    let att = owner.attest_agent(agent.did());
                    att.save(&att_path)?;
                    println!("  Attested agent: {}", agent.did());
                    println!("  Attestation:    {}", att_path.display());
                }
            }
        }

        Commands::Project { action } => {
            let api_base = "http://127.0.0.1:7313";
            let client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(5))
                .build()
                .unwrap();
            let daemon_running = client
                .get(format!("{}/status", api_base))
                .send()
                .await
                .is_ok();
            if !daemon_running {
                eprintln!("Daemon must be running for project commands.");
                std::process::exit(1);
            }

            match action {
                ProjectAction::List => {
                    match client.get(format!("{}/projects", api_base)).send().await {
                        Ok(resp) if resp.status().is_success() => {
                            let data: serde_json::Value = resp.json().await.unwrap_or_default();
                            if cli.format == "json" {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&data).unwrap_or_default()
                                );
                            } else {
                                let projects = data
                                    .get("projects")
                                    .and_then(|v| v.as_array())
                                    .cloned()
                                    .unwrap_or_default();
                                if projects.is_empty() {
                                    println!(
                                        "  No projects. Create one with: agora project create <name>"
                                    );
                                } else {
                                    println!();
                                    let headers = vec!["ID", "Name", "Status", "Stage", "Agents"];
                                    let rows: Vec<Vec<String>> = projects
                                        .iter()
                                        .map(|p| {
                                            let name = p
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("?");
                                            let status = p
                                                .get("status")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("?");
                                            let stage = p
                                                .get("current_stage")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("-");
                                            let agents = p
                                                .get("agent_count")
                                                .and_then(|v| v.as_u64())
                                                .unwrap_or(0);
                                            let active = p
                                                .get("active_agents")
                                                .and_then(|v| v.as_u64())
                                                .unwrap_or(0);
                                            let id =
                                                p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                                            vec![
                                                format::short_id(id),
                                                name.to_string(),
                                                match status {
                                                    "active" => format::green(status),
                                                    "paused" => format::yellow(status),
                                                    "completed" => format::dim(status),
                                                    _ => status.to_string(),
                                                },
                                                stage.to_string(),
                                                format!("{}/{}", active, agents),
                                            ]
                                        })
                                        .collect();
                                    println!("  Projects ({}):", projects.len());
                                    println!();
                                    format::print_table(&headers, &rows);
                                    println!();
                                }
                            }
                        }
                        _ => {
                            eprintln!("Failed to fetch projects.");
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::Create {
                    name,
                    repo,
                    description,
                } => {
                    let body = serde_json::json!({ "name": name, "description": description, "repo": repo });
                    match client
                        .post(format!("{}/projects", api_base))
                        .json(&body)
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            let data: serde_json::Value = resp.json().await.unwrap_or_default();
                            let id = data.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                            println!("Created project: {} (ID: {})", name, id);
                        }
                        _ => {
                            eprintln!("Failed to create project.");
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::Show { id } => {
                    match client
                        .get(format!("{}/projects/{}", api_base, id))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            let data: serde_json::Value = resp.json().await.unwrap_or_default();
                            if cli.format == "json" {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&data).unwrap_or_default()
                                );
                            } else {
                                let name = data.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                                let status =
                                    data.get("status").and_then(|v| v.as_str()).unwrap_or("?");
                                let stage = data.get("current_stage").and_then(|v| v.as_str());
                                let desc = data.get("description").and_then(|v| v.as_str());
                                let proj_id =
                                    data.get("id").and_then(|v| v.as_str()).unwrap_or("?");

                                println!();
                                println!("  {}", format::bold(name));
                                println!();
                                format::print_kv("ID", proj_id);
                                format::print_kv("Status", status);
                                if let Some(d) = desc {
                                    format::print_kv("Description", d);
                                }
                                if let Some(s) = stage {
                                    format::print_kv("Stage", &format::stage_bar(Some(s)));
                                }

                                // Agents table
                                let agents = data
                                    .get("agents")
                                    .and_then(|v| v.as_array())
                                    .cloned()
                                    .unwrap_or_default();
                                if !agents.is_empty() {
                                    println!();
                                    println!("  {} Agents:", format::bold(""));
                                    let headers = vec!["Name", "Role", "Status", "Focus"];
                                    let rows: Vec<Vec<String>> = agents
                                        .iter()
                                        .map(|a| {
                                            let aname = a
                                                .get("name")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("?");
                                            let role = a
                                                .get("role")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("?");
                                            let clocked = a
                                                .get("clocked_in")
                                                .and_then(|v| v.as_bool())
                                                .unwrap_or(false);
                                            let focus = a
                                                .get("current_focus")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("-");
                                            let suspended = a
                                                .get("suspended")
                                                .and_then(|v| v.as_bool())
                                                .unwrap_or(false);
                                            vec![
                                                aname.to_string(),
                                                role.to_string(),
                                                if suspended {
                                                    format::red("suspended")
                                                } else if clocked {
                                                    format::green("active")
                                                } else {
                                                    format::dim("idle")
                                                },
                                                focus.to_string(),
                                            ]
                                        })
                                        .collect();
                                    format::print_table(&headers, &rows);
                                }

                                // Task summary
                                let tasks =
                                    data.get("task_count").and_then(|v| v.as_u64()).unwrap_or(0);
                                let done =
                                    data.get("tasks_done").and_then(|v| v.as_u64()).unwrap_or(0);
                                if tasks > 0 {
                                    println!();
                                    format::print_kv("Tasks", &format!("{}/{} done", done, tasks));
                                }
                                println!();
                            }
                        }
                        Ok(resp) if resp.status() == reqwest::StatusCode::NOT_FOUND => {
                            eprintln!("Project not found: {}", id);
                        }
                        _ => {
                            eprintln!("Failed to fetch project.");
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::Invite {
                    project_id,
                    peer_name,
                    role,
                    message,
                } => {
                    let body = serde_json::json!({
                        "project_id": project_id,
                        "peer_name": peer_name,
                        "role": role,
                        "message": message,
                    });
                    match client
                        .post(format!("{}/project-invitations", api_base))
                        .json(&body)
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            println!("Invited {} to project as {}", peer_name, role);
                        }
                        _ => {
                            eprintln!("Failed to send invitation.");
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::Join { invitation_id } => {
                    match client
                        .post(format!(
                            "{}/project-invitations/{}/accept",
                            api_base, invitation_id
                        ))
                        .json(&serde_json::json!({}))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            println!("Accepted project invitation.");
                        }
                        _ => {
                            eprintln!("Failed to accept invitation.");
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::Leave { project_id } => {
                    use crate::protocol::message::ProjectLeavePayload;
                    let uuid = uuid::Uuid::parse_str(&project_id).unwrap_or_else(|_| {
                        eprintln!("Invalid project ID");
                        std::process::exit(1);
                    });
                    let payload = ProjectLeavePayload {
                        project_id: uuid,
                        reason: None,
                    };
                    let body = serde_json::to_string(&payload).unwrap_or_default();
                    // Send leave message via outbox
                    let send_body = serde_json::json!({ "body": body });
                    let _ = client
                        .post(format!("{}/send", api_base))
                        .json(&send_body)
                        .send()
                        .await;
                    println!("Left project {}", project_id);
                }
                ProjectAction::ClockIn { project_id, focus } => {
                    let body = serde_json::json!({ "focus": focus });
                    match client
                        .post(format!("{}/projects/{}/clock-in", api_base, project_id))
                        .json(&body)
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            println!("Clocked in to project {}", project_id);
                            if let Some(ref f) = focus {
                                println!("  Focus: {}", f);
                            }
                        }
                        _ => {
                            eprintln!("Failed to clock in.");
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::ClockOut { project_id } => {
                    match client
                        .post(format!("{}/projects/{}/clock-out", api_base, project_id))
                        .json(&serde_json::json!({}))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            println!("Clocked out of project {}", project_id);
                        }
                        _ => {
                            eprintln!("Failed to clock out.");
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::Tasks { project_id } => {
                    match client
                        .get(format!("{}/projects/{}/tasks", api_base, project_id))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            let data: serde_json::Value = resp.json().await.unwrap_or_default();
                            if cli.format == "json" {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&data).unwrap_or_default()
                                );
                            } else {
                                let tasks = data
                                    .get("tasks")
                                    .and_then(|v| v.as_array())
                                    .cloned()
                                    .unwrap_or_default();
                                if tasks.is_empty() {
                                    println!(
                                        "  No tasks. Add one with: agora project add-task {} <title>",
                                        project_id
                                    );
                                } else {
                                    println!();
                                    let headers =
                                        vec!["", "Title", "Assignee", "Status", "Priority"];
                                    let rows: Vec<Vec<String>> = tasks
                                        .iter()
                                        .map(|t| {
                                            let status = t
                                                .get("status")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("todo");
                                            let title = t
                                                .get("title")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("?");
                                            let assignee = t
                                                .get("assignee")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("-");
                                            let priority = t
                                                .get("priority")
                                                .and_then(|v| v.as_str())
                                                .unwrap_or("-");
                                            vec![
                                                format::task_status_icon(status).to_string(),
                                                title.to_string(),
                                                assignee.to_string(),
                                                match status {
                                                    "done" => format::green(status),
                                                    "in_progress" => format::yellow(status),
                                                    "blocked" => format::red(status),
                                                    _ => status.to_string(),
                                                },
                                                match priority {
                                                    "critical" => format::red(priority),
                                                    "high" => format::yellow(priority),
                                                    _ => priority.to_string(),
                                                },
                                            ]
                                        })
                                        .collect();
                                    format::print_table(&headers, &rows);
                                    println!();
                                }
                            }
                        }
                        _ => {
                            eprintln!("Failed to list tasks.");
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::AddTask {
                    project_id,
                    title,
                    description,
                    assignee,
                    priority,
                } => {
                    let body = serde_json::json!({
                        "title": title,
                        "description": description,
                        "assignee": assignee,
                        "priority": priority,
                    });
                    match client
                        .post(format!("{}/projects/{}/tasks", api_base, project_id))
                        .json(&body)
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            let body = resp.text().await.unwrap_or_default();
                            println!("{}", body);
                        }
                        _ => {
                            eprintln!("Failed to create task.");
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::UpdateTask {
                    project_id,
                    task_id,
                    status,
                    assignee,
                } => {
                    let body = serde_json::json!({
                        "status": status,
                        "assignee": assignee,
                    });
                    match client
                        .patch(format!(
                            "{}/projects/{}/tasks/{}",
                            api_base, project_id, task_id
                        ))
                        .json(&body)
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            let body = resp.text().await.unwrap_or_default();
                            println!("{}", body);
                        }
                        _ => {
                            eprintln!("Failed to update task.");
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::Stage {
                    project_id,
                    stage,
                    advance,
                } => {
                    if advance || stage.is_some() {
                        let body = serde_json::json!({
                            "stage": stage,
                            "advance": advance,
                        });
                        match client
                            .post(format!("{}/projects/{}/stage", api_base, project_id))
                            .json(&body)
                            .send()
                            .await
                        {
                            Ok(resp) if resp.status().is_success() => {
                                let data: serde_json::Value = resp.json().await.unwrap_or_default();
                                if cli.format == "json" {
                                    println!(
                                        "{}",
                                        serde_json::to_string_pretty(&data).unwrap_or_default()
                                    );
                                } else {
                                    let s = data.get("current_stage").and_then(|v| v.as_str());
                                    println!();
                                    println!("  {}", format::stage_bar(s));
                                    println!();
                                }
                            }
                            Ok(resp) => {
                                let text = resp.text().await.unwrap_or_default();
                                eprintln!("Failed to set/advance stage: {}", text);
                                std::process::exit(1);
                            }
                            Err(e) => {
                                eprintln!("Failed to set/advance stage: {}", e);
                                std::process::exit(1);
                            }
                        }
                    } else {
                        match client
                            .get(format!("{}/projects/{}/stage", api_base, project_id))
                            .send()
                            .await
                        {
                            Ok(resp) if resp.status().is_success() => {
                                let data: serde_json::Value = resp.json().await.unwrap_or_default();
                                if cli.format == "json" {
                                    println!(
                                        "{}",
                                        serde_json::to_string_pretty(&data).unwrap_or_default()
                                    );
                                } else {
                                    let s = data.get("current_stage").and_then(|v| v.as_str());
                                    println!();
                                    println!("  {}", format::stage_bar(s));
                                    println!();
                                }
                            }
                            _ => {
                                eprintln!("Failed to get stage.");
                                std::process::exit(1);
                            }
                        }
                    }
                }
                ProjectAction::Audit { project_id, limit } => {
                    let limit_q = if limit > 0 {
                        format!("?limit={}", limit)
                    } else {
                        String::new()
                    };
                    match client
                        .get(format!(
                            "{}/projects/{}/audit{}",
                            api_base, project_id, limit_q
                        ))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            let data: serde_json::Value = resp.json().await.unwrap_or_default();
                            if cli.format == "json" {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&data).unwrap_or_default()
                                );
                            } else {
                                let entries = data
                                    .get("entries")
                                    .and_then(|v| v.as_array())
                                    .cloned()
                                    .unwrap_or_default();
                                if entries.is_empty() {
                                    println!("  No audit entries.");
                                } else {
                                    println!();
                                    let headers = vec!["Time", "Author", "Action", "Detail"];
                                    let rows: Vec<Vec<String>> = entries
                                        .iter()
                                        .map(|e| {
                                            vec![
                                                e.get("timestamp")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("?")
                                                    .chars()
                                                    .take(19)
                                                    .collect(),
                                                e.get("author_name")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("?")
                                                    .to_string(),
                                                e.get("action")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("?")
                                                    .to_string(),
                                                e.get("detail")
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("")
                                                    .to_string(),
                                            ]
                                        })
                                        .collect();
                                    format::print_table(&headers, &rows);
                                    println!();
                                }
                            }
                        }
                        _ => {
                            eprintln!("Failed to fetch audit trail.");
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::Suspend {
                    project_id,
                    agent_name,
                    reason,
                } => {
                    let body = serde_json::json!({ "reason": reason });
                    match client
                        .post(format!(
                            "{}/projects/{}/agents/{}/suspend",
                            api_base, project_id, agent_name
                        ))
                        .json(&body)
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            println!("  {} Agent '{}' suspended", format::yellow("⚠"), agent_name);
                        }
                        Ok(resp) => {
                            let text = resp.text().await.unwrap_or_default();
                            eprintln!("Failed to suspend agent: {}", text);
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Failed to suspend agent: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::Unsuspend {
                    project_id,
                    agent_name,
                } => {
                    match client
                        .post(format!(
                            "{}/projects/{}/agents/{}/unsuspend",
                            api_base, project_id, agent_name
                        ))
                        .json(&serde_json::json!({}))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            println!(
                                "  {} Agent '{}' unsuspended",
                                format::green("✓"),
                                agent_name
                            );
                        }
                        Ok(resp) => {
                            let text = resp.text().await.unwrap_or_default();
                            eprintln!("Failed to unsuspend agent: {}", text);
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Failed to unsuspend agent: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::Conversation { project_id, limit } => {
                    match client
                        .get(format!(
                            "{}/projects/{}/conversations",
                            api_base, project_id
                        ))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            let data: serde_json::Value = resp.json().await.unwrap_or_default();
                            if cli.format == "json" {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&data).unwrap_or_default()
                                );
                            } else {
                                let messages = data
                                    .get("messages")
                                    .and_then(|v| v.as_array())
                                    .cloned()
                                    .unwrap_or_default();
                                let total = messages.len();
                                let display: Vec<&serde_json::Value> = if limit > 0 && total > limit
                                {
                                    messages.iter().skip(total - limit).collect()
                                } else {
                                    messages.iter().collect()
                                };
                                if display.is_empty() {
                                    println!("  No project messages yet.");
                                } else {
                                    println!();
                                    println!(
                                        "  {} ({} messages)",
                                        format::bold("Project Conversation"),
                                        total
                                    );
                                    println!();
                                    for msg in display {
                                        let from =
                                            msg.get("from").and_then(|v| v.as_str()).unwrap_or("?");
                                        let body =
                                            msg.get("body").and_then(|v| v.as_str()).unwrap_or("");
                                        let ts = msg
                                            .get("timestamp")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        let dir = msg
                                            .get("direction")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("");
                                        let arrow = if dir == "outbound" { ">>>" } else { "<<<" };
                                        let preview: String = body.chars().take(120).collect();
                                        println!(
                                            "  {} {} {} {}",
                                            format::dim(ts),
                                            arrow,
                                            format::bold(from),
                                            preview
                                        );
                                    }
                                    println!();
                                }
                            }
                        }
                        Ok(resp) => {
                            let text = resp.text().await.unwrap_or_default();
                            eprintln!("Failed to get project conversations: {}", text);
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Failed to get project conversations: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::GithubSync { project_id } => {
                    match client
                        .post(format!("{}/projects/{}/github/sync", api_base, project_id))
                        .header("Content-Type", "application/json")
                        .body("{}")
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            let data: serde_json::Value = resp.json().await.unwrap_or_default();
                            if cli.format == "json" {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&data).unwrap_or_default()
                                );
                            } else {
                                let imported =
                                    data.get("imported").and_then(|v| v.as_u64()).unwrap_or(0);
                                let pushed =
                                    data.get("pushed").and_then(|v| v.as_u64()).unwrap_or(0);
                                let errors = data
                                    .get("errors")
                                    .and_then(|v| v.as_array())
                                    .map(|a| a.len())
                                    .unwrap_or(0);
                                println!();
                                println!("  {} GitHub Sync Complete", format::bold(">>>"));
                                println!("  Imported: {} issues", imported);
                                println!("  Pushed:   {} tasks", pushed);
                                if errors > 0 {
                                    println!("  Errors:   {}", errors);
                                }
                                println!();
                            }
                        }
                        Ok(resp) => {
                            let text = resp.text().await.unwrap_or_default();
                            eprintln!("GitHub sync failed: {}", text);
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("GitHub sync failed: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::GithubToken { token } => {
                    match client
                        .post(format!("{}/github/config", api_base))
                        .json(&serde_json::json!({ "token": token }))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            println!("  GitHub token saved.");
                        }
                        Ok(resp) => {
                            let text = resp.text().await.unwrap_or_default();
                            eprintln!("Failed to save token: {}", text);
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Failed to save token: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
                ProjectAction::GithubStatus { project_id } => {
                    match client
                        .get(format!(
                            "{}/projects/{}/github/status",
                            api_base, project_id
                        ))
                        .send()
                        .await
                    {
                        Ok(resp) if resp.status().is_success() => {
                            let data: serde_json::Value = resp.json().await.unwrap_or_default();
                            if cli.format == "json" {
                                println!(
                                    "{}",
                                    serde_json::to_string_pretty(&data).unwrap_or_default()
                                );
                            } else {
                                let has_token = data
                                    .get("has_token")
                                    .and_then(|v| v.as_bool())
                                    .unwrap_or(false);
                                let repo = data
                                    .get("parsed_repo")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("none");
                                let linked = data
                                    .get("github_linked_tasks")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0);
                                let local = data
                                    .get("local_only_tasks")
                                    .and_then(|v| v.as_u64())
                                    .unwrap_or(0);
                                println!();
                                println!("  {} GitHub Integration", format::bold(">>>"));
                                println!(
                                    "  Token:  {}",
                                    if has_token { "configured" } else { "not set" }
                                );
                                println!("  Repo:   {}", repo);
                                println!("  Linked: {} tasks", linked);
                                println!("  Local:  {} tasks (not synced)", local);
                                println!();
                            }
                        }
                        Ok(resp) => {
                            let text = resp.text().await.unwrap_or_default();
                            eprintln!("Failed to get GitHub status: {}", text);
                            std::process::exit(1);
                        }
                        Err(e) => {
                            eprintln!("Failed to get GitHub status: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
            }
        }

        Commands::Token { action } => {
            let token_path = auth::default_token_path();
            match action {
                TokenAction::Show => {
                    let token = auth::load_or_create_token(&token_path);
                    if cli.format == "json" {
                        println!("{}", serde_json::json!({ "token": token }));
                    } else {
                        println!("{}", format::bold("API Token"));
                        println!("  {}", token);
                        println!();
                        println!("Use this token in the dashboard login page.");
                        println!("Path: {}", token_path.display());
                    }
                }
                TokenAction::Regenerate => {
                    let token = auth::regenerate_token(&token_path);
                    if cli.format == "json" {
                        println!(
                            "{}",
                            serde_json::json!({ "token": token, "regenerated": true })
                        );
                    } else {
                        println!("{} New API token generated", format::bold("OK"));
                        println!("  {}", token);
                        println!();
                        println!(
                            "The old token is now invalid. Restart the daemon for the change to take effect."
                        );
                    }
                }
            }
        }

        Commands::SetupClaude {
            agent_name,
            api_port,
            dir,
        } => {
            let target_dir = dir
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| ".".into()));

            // Find the agora binary path
            let agora_bin =
                std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("agora"));

            // Generate .mcp.json
            let mcp_json = serde_json::json!({
                "mcpServers": {
                    "agora": {
                        "type": "stdio",
                        "command": agora_bin.to_string_lossy(),
                        "args": ["mcp", "--api-port", api_port.to_string(), "--agent-name", &agent_name]
                    }
                }
            });

            // Generate .claude/settings.local.json with all agora MCP tools pre-allowed
            let settings_json = serde_json::json!({
                "permissions": {
                    "allow": [
                        "mcp__agora__agora_status",
                        "mcp__agora__agora_identity",
                        "mcp__agora__agora_list_peers",
                        "mcp__agora__agora_read_messages",
                        "mcp__agora__agora_send_message",
                        "mcp__agora__agora_send_to_room",
                        "mcp__agora__agora_list_friends",
                        "mcp__agora__agora_add_friend",
                        "mcp__agora__agora_remove_friend",
                        "mcp__agora__agora_friend_requests",
                        "mcp__agora__agora_send_friend_request",
                        "mcp__agora__agora_respond_friend_request",
                        "mcp__agora__agora_get_wake",
                        "mcp__agora__agora_set_wake",
                        "mcp__agora__agora_get_conversation",
                        "mcp__agora__agora_projects",
                        "mcp__agora__agora_create_project",
                        "mcp__agora__agora_invite_to_project",
                        "mcp__agora__agora_respond_project_invitation",
                        "mcp__agora__agora_project_clock",
                        "mcp__agora__agora_project_tasks",
                        "mcp__agora__agora_project_audit",
                        "mcp__agora__agora_project_stage",
                        "mcp__agora__agora_project_oversight",
                        "mcp__agora__agora_project_conversations",
                        "mcp__agora__agora_github_sync",
                        "mcp__agora__agora_github_config"
                    ]
                },
                "enabledMcpjsonServers": ["agora"]
            });

            // Write .mcp.json
            let mcp_path = target_dir.join(".mcp.json");
            let mcp_content = serde_json::to_string_pretty(&mcp_json).unwrap();
            std::fs::write(&mcp_path, format!("{}\n", mcp_content))
                .map_err(|e| anyhow::anyhow!("Failed to write {}: {}", mcp_path.display(), e))?;

            // Write .claude/settings.local.json
            let claude_dir = target_dir.join(".claude");
            std::fs::create_dir_all(&claude_dir)
                .map_err(|e| anyhow::anyhow!("Failed to create {}: {}", claude_dir.display(), e))?;
            let settings_path = claude_dir.join("settings.local.json");
            let settings_content = serde_json::to_string_pretty(&settings_json).unwrap();
            std::fs::write(&settings_path, format!("{}\n", settings_content)).map_err(|e| {
                anyhow::anyhow!("Failed to write {}: {}", settings_path.display(), e)
            })?;

            if cli.format == "json" {
                println!(
                    "{}",
                    serde_json::json!({
                        "mcp_json": mcp_path.to_string_lossy(),
                        "settings_json": settings_path.to_string_lossy(),
                        "agent_name": agent_name,
                        "api_port": api_port,
                    })
                );
            } else {
                println!("{} Claude Code config generated", format::bold("OK"));
                println!();
                println!("  {} {}", format::bold("MCP config:"), mcp_path.display());
                println!(
                    "  {} {}",
                    format::bold("Permissions:"),
                    settings_path.display()
                );
                println!("  {} {}", format::bold("Agent name:"), agent_name);
                println!("  {} {}", format::bold("API port:"), api_port);
                println!();
                println!("All agora MCP tools are pre-allowed — background sub-agents");
                println!("(like the Agora listener) will not be blocked by permission prompts.");
                println!();
                println!("To start Claude Code in this directory:");
                println!("  cd {} && claude", target_dir.display());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_send_as_defaults_to_listener_prefix() {
        assert_eq!(
            default_listener_send_as("claude", Some("codex-listener")),
            "codex"
        );
        assert_eq!(
            default_listener_send_as("claude", Some("codex-review-listener")),
            "codex-review"
        );
    }

    #[test]
    fn listener_send_as_falls_back_to_cli_name() {
        assert_eq!(default_listener_send_as("claude", None), "claude");
        assert_eq!(
            default_listener_send_as("claude", Some("listener-without-suffix")),
            "claude"
        );
    }
}
