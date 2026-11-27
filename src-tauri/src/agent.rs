//! The agent loop: stream a turn, run the requested tools (asking the user
//! when the permission mode says so), feed results back, and repeat until the
//! model is done. Conversations persist to disk as JSON.

use crate::providers::{self, TurnRequest};
use crate::settings::{self, PermissionMode};
use crate::tools::{self, Checkpoint, Risk, ToolCtx};
use crate::types::*;
use anyhow::{anyhow, Result};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

const MAX_STEPS: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    #[serde(default)]
    pub files: HashMap<String, Option<String>>,
    #[serde(default)]
    pub reverted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub project: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(default)]
    pub runs: Vec<Run>,
    #[serde(default)]
    pub usage: Usage,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub project: String,
    pub title: String,
    pub updated_at: i64,
    pub model: String,
}

fn sessions_dir() -> PathBuf {
    let d = settings::data_dir().join("sessions");
    let _ = std::fs::create_dir_all(&d);
    d
}

fn session_path(id: &str) -> Result<PathBuf> {
    if !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(anyhow!("Bad session id"));
    }
    Ok(sessions_dir().join(format!("{id}.json")))
}

pub fn load_session(id: &str) -> Result<Session> {
    Ok(serde_json::from_str(&std::fs::read_to_string(session_path(id)?)?)?)
}

pub fn save_session(s: &Session) -> Result<()> {
    let p = session_path(&s.id)?;
    let tmp = p.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(s)?)?;
    std::fs::rename(tmp, p)?;
    Ok(())
}

pub fn delete_session(id: &str) -> Result<()> {
    let _ = std::fs::remove_file(session_path(id)?);
    Ok(())
}

pub fn list_sessions(project: Option<&str>) -> Vec<SessionSummary> {
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(sessions_dir()) else { return out };
    for e in rd.flatten() {
        if e.path().extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
        // Parse only the light fields.
        #[derive(Deserialize)]
        struct Head {
            id: String,
            project: String,
            title: String,
            updated_at: i64,
            model: String,
        }
        if let Ok(h) = serde_json::from_str::<Head>(&text) {
            if project.map_or(true, |p| p == h.project) {
                out.push(SessionSummary { id: h.id, project: h.project, title: h.title, updated_at: h.updated_at, model: h.model });
            }
        }
    }
    out.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    out
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Allow,
    AllowAlways,
    Deny,
}

#[derive(Default)]
pub struct AgentState {
    pub running: Mutex<HashMap<String, CancellationToken>>,
    pub approvals: Mutex<HashMap<String, oneshot::Sender<Decision>>>,
}

#[derive(Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentEvent {
    Started { session_id: String, run_id: String },
    TextDelta { session_id: String, text: String },
    ThinkingDelta { session_id: String, text: String },
    ToolStarted { session_id: String, name: String },
    Message { session_id: String, message: Message },
    ToolRunning { session_id: String, tool_use_id: String },
    ToolResult { session_id: String, block: Block },
    Approval { session_id: String, request_id: String, tool_use_id: String, name: String, input: Value },
    Usage { session_id: String, turn: Usage, total: Usage },
    Done { session_id: String, run_id: String, files_changed: usize, stopped: bool },
    Error { session_id: String, message: String },
}

fn emit(app: &AppHandle, ev: AgentEvent) {
    let _ = app.emit("agent", ev);
}

fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn title_from(text: &str) -> String {
    let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() > 60 {
        format!("{}…", t.chars().take(58).collect::<String>())
    } else if t.is_empty() {
        "New chat".into()
    } else {
        t
    }
}

pub fn new_session(project: &str, provider: &str, model: &str) -> Session {
    Session {
        id: uuid::Uuid::new_v4().to_string(),
        project: project.to_string(),
        title: "New chat".into(),
        created_at: now(),
        updated_at: now(),
        provider: provider.into(),
        model: model.into(),
        messages: vec![],
        runs: vec![],
        usage: Usage::default(),
    }
}

fn system_prompt(root: &Path, disabled: &[String]) -> String {
    let os = std::env::consts::OS;
    let date = chrono::Utc::now().format("%Y-%m-%d");
    let mut s = format!(
        "You are Casylum, an expert software engineer working inside the user's project on their computer.\n\
         Project root: {root}\nOS: {os}\nDate: {date}\n\n\
         How to work:\n\
         - Explore before changing: use list_dir, search and read_file to understand the code.\n\
         - For multi-step tasks, call update_plan first and keep it current. Give each step a short title and a one-sentence description.\n\
         - Make focused changes with edit_file; use write_file for new files.\n\
         - Verify your work with run_command (build, tests, linters) when the project has them.\n\
         - Paths are relative to the project root. Never touch files outside it.\n\
         - Be concise. When done, summarize what changed and anything the user should check.\n",
        root = root.display()
    );
    for name in ["CASYLUM.md", "AGENTS.md", "CLAUDE.md"] {
        if let Ok(text) = std::fs::read_to_string(root.join(name)) {
            let text: String = text.chars().take(20_000).collect();
            s.push_str(&format!("\nProject instructions from {name}:\n<project_instructions>\n{text}\n</project_instructions>\n"));
            break;
        }
    }
    let skills: Vec<_> = crate::skills::list(Some(&root.to_string_lossy()), disabled).into_iter().filter(|s| s.enabled).collect();
    if !skills.is_empty() {
        s.push_str("\nAvailable skills. When a task matches one, call use_skill with its name before starting:\n");
        for sk in skills {
            s.push_str(&format!("- {}: {}\n", sk.name, sk.description));
        }
    }
    s
}

const NO_FOLDER: &str = "No project folder is open in this chat, so file and shell tools are unavailable. Ask the user to open a folder.";

fn chat_prompt() -> String {
    let os = std::env::consts::OS;
    let date = chrono::Utc::now().format("%Y-%m-%d");
    format!(
        "You are Casylum, an expert software engineer and helpful assistant.\n\
         OS: {os}\nDate: {date}\n\n\
         No project folder is open in this chat, so you cannot read files, edit code or run commands. \
         Answer questions, explain, and write code in your replies. If the user asks for something that needs their files, \
         tell them to open a folder with the folder button at the top of the sidebar (or Ctrl+O) and start a chat there.\n\
         Be concise.\n"
    )
}

pub struct SendArgs {
    pub session_id: String,
    pub text: String,
    pub provider: String,
    pub model: String,
}

pub async fn run_agent(app: AppHandle, state: Arc<AgentState>, http: reqwest::Client, args: SendArgs) {
    let sid = args.session_id.clone();
    let cancel = CancellationToken::new();
    {
        let mut running = state.running.lock();
        if running.contains_key(&sid) {
            emit(&app, AgentEvent::Error { session_id: sid, message: "This chat is already running.".into() });
            return;
        }
        running.insert(sid.clone(), cancel.clone());
    }
    let run_id = uuid::Uuid::new_v4().to_string();
    emit(&app, AgentEvent::Started { session_id: sid.clone(), run_id: run_id.clone() });

    let result = drive(&app, &state, &http, &args, &run_id, &cancel).await;
    let (files_changed, err) = match result {
        Ok(n) => (n, None),
        Err(e) => (0, Some(e.to_string())),
    };
    if let Some(message) = err {
        emit(&app, AgentEvent::Error { session_id: sid.clone(), message });
    }
    state.running.lock().remove(&sid);
    emit(&app, AgentEvent::Done { session_id: sid, run_id, files_changed, stopped: cancel.is_cancelled() });
}

async fn drive(
    app: &AppHandle,
    state: &Arc<AgentState>,
    http: &reqwest::Client,
    args: &SendArgs,
    run_id: &str,
    cancel: &CancellationToken,
) -> Result<usize> {
    let sid = &args.session_id;
    let mut session = load_session(sid)?;
    // A chat without a folder is plain conversation: no file or shell tools.
    let root = (!session.project.is_empty()).then(|| PathBuf::from(&session.project));
    if let Some(root) = &root {
        if !root.is_dir() {
            return Err(anyhow!("Project folder not found: {}", root.display()));
        }
    }
    session.provider = args.provider.clone();
    session.model = args.model.clone();
    if session.messages.is_empty() {
        session.title = title_from(&args.text);
    }
    // Merge into a trailing user message (e.g. after an error) to keep roles alternating.
    match session.messages.last_mut() {
        Some(m) if m.role == Role::User => m.content.push(Block::Text { text: args.text.clone() }),
        _ => session.messages.push(Message::user_text(&args.text)),
    }
    session.updated_at = now();
    save_session(&session)?;
    emit(app, AgentEvent::Message { session_id: sid.clone(), message: session.messages.last().unwrap().clone() });

    let specs = if root.is_some() { tools::specs() } else { vec![] };
    let mut checkpoint: Checkpoint = HashMap::new();
    let mut always: HashSet<String> = HashSet::new();

    let finish = |session: &mut Session, checkpoint: &Checkpoint| -> Result<usize> {
        if !checkpoint.is_empty() {
            session.runs.push(Run {
                id: run_id.to_string(),
                files: checkpoint.iter().map(|(k, v)| (k.to_string_lossy().to_string(), v.clone())).collect(),
                reverted: false,
            });
        }
        session.updated_at = now();
        save_session(session)?;
        Ok(checkpoint.len())
    };

    for _ in 0..MAX_STEPS {
        if cancel.is_cancelled() {
            return finish(&mut session, &checkpoint);
        }
        let settings = settings::load();
        let base_url = match args.provider.as_str() {
            "ollama" => Some(settings.ollama_base_url.clone()),
            "custom" => Some(settings.custom_base_url.clone()),
            _ => None,
        };
        let system = match &root {
            Some(root) => system_prompt(root, &settings.disabled_skills),
            None => chat_prompt(),
        };
        let req = TurnRequest {
            provider: &args.provider,
            model: &args.model,
            system: &system,
            messages: &session.messages,
            tools: &specs,
            api_key: crate::keys::get(&args.provider),
            base_url,
        };
        let app2 = app.clone();
        let sid2 = sid.clone();
        let mut on_delta = move |d: StreamDelta| {
            let ev = match d {
                StreamDelta::Text(text) => AgentEvent::TextDelta { session_id: sid2.clone(), text },
                StreamDelta::Thinking(text) => AgentEvent::ThinkingDelta { session_id: sid2.clone(), text },
                StreamDelta::ToolStarted { name } => AgentEvent::ToolStarted { session_id: sid2.clone(), name },
            };
            emit(&app2, ev);
        };
        let turn = tokio::select! {
            r = providers::stream_turn(http, req, &mut on_delta) => r,
            _ = cancel.cancelled() => return finish(&mut session, &checkpoint),
        };
        let turn = match turn {
            Ok(t) => t,
            Err(e) => {
                finish(&mut session, &checkpoint)?;
                return Err(e);
            }
        };

        session.usage.input_tokens += turn.usage.input_tokens;
        session.usage.output_tokens += turn.usage.output_tokens;
        emit(app, AgentEvent::Usage { session_id: sid.clone(), turn: turn.usage.clone(), total: session.usage.clone() });

        if turn.content.is_empty() {
            if turn.stop == StopReason::Refusal {
                finish(&mut session, &checkpoint)?;
                return Err(anyhow!("The model declined this request."));
            }
            break;
        }
        let assistant = Message { role: Role::Assistant, content: turn.content.clone() };
        session.messages.push(assistant.clone());
        emit(app, AgentEvent::Message { session_id: sid.clone(), message: assistant });

        let uses: Vec<(String, String, Value)> = turn
            .content
            .iter()
            .filter_map(|b| match b {
                Block::ToolUse { id, name, input, .. } => Some((id.clone(), name.clone(), input.clone())),
                _ => None,
            })
            .collect();
        if uses.is_empty() {
            if turn.stop == StopReason::Refusal {
                finish(&mut session, &checkpoint)?;
                return Err(anyhow!("The model declined this request."));
            }
            break;
        }

        let mut results = vec![];
        for (id, name, input) in uses {
            if cancel.is_cancelled() || turn.stop == StopReason::MaxTokens {
                let why = if cancel.is_cancelled() { "Stopped by the user." } else { "Output limit reached before this call finished; retry with a smaller step." };
                results.push(Block::ToolResult { tool_use_id: id, content: why.into(), is_error: true, meta: None });
                continue;
            }
            let mode = settings::load().permission_mode;
            let needs_ok = match (tools::risk(&name), mode) {
                (Risk::Read, _) => false,
                (_, PermissionMode::FullAuto) => false,
                (Risk::Edit, PermissionMode::AutoEdit) => false,
                _ => !always.contains(&name),
            };
            if needs_ok {
                let request_id = uuid::Uuid::new_v4().to_string();
                let (tx, rx) = oneshot::channel();
                state.approvals.lock().insert(request_id.clone(), tx);
                emit(app, AgentEvent::Approval { session_id: sid.clone(), request_id: request_id.clone(), tool_use_id: id.clone(), name: name.clone(), input: input.clone() });
                let decision = tokio::select! {
                    d = rx => d.unwrap_or(Decision::Deny),
                    _ = cancel.cancelled() => Decision::Deny,
                };
                state.approvals.lock().remove(&request_id);
                match decision {
                    Decision::Deny => {
                        let msg = if cancel.is_cancelled() { "Stopped by the user." } else { "The user declined this action. Ask them how to proceed or try a different approach." };
                        results.push(Block::ToolResult { tool_use_id: id, content: msg.into(), is_error: true, meta: None });
                        continue;
                    }
                    Decision::AllowAlways => {
                        always.insert(name.clone());
                    }
                    Decision::Allow => {}
                }
            }
            let Some(root) = &root else {
                results.push(Block::ToolResult { tool_use_id: id, content: NO_FOLDER.into(), is_error: true, meta: None });
                continue;
            };
            emit(app, AgentEvent::ToolRunning { session_id: sid.clone(), tool_use_id: id.clone() });
            let mut ctx = ToolCtx { root, disabled_skills: &settings::load().disabled_skills, checkpoint: &mut checkpoint };
            let out = tools::run(&name, &input, &mut ctx).await;
            let block = Block::ToolResult { tool_use_id: id, content: out.content, is_error: out.is_error, meta: out.meta };
            emit(app, AgentEvent::ToolResult { session_id: sid.clone(), block: block.clone() });
            results.push(block);
        }
        let msg = Message { role: Role::User, content: results };
        session.messages.push(msg.clone());
        session.updated_at = now();
        save_session(&session)?;
        emit(app, AgentEvent::Message { session_id: sid.clone(), message: msg });
    }
    finish(&mut session, &checkpoint)
}

pub fn revert_run(session_id: &str, run_id: &str) -> Result<Vec<String>> {
    let mut s = load_session(session_id)?;
    let run = s.runs.iter_mut().find(|r| r.id == run_id).ok_or_else(|| anyhow!("Nothing to undo for that run"))?;
    if run.reverted {
        return Err(anyhow!("That run was already undone"));
    }
    let cp: Checkpoint = run.files.iter().map(|(k, v)| (PathBuf::from(k), v.clone())).collect();
    let restored = tools::restore(&cp);
    run.reverted = true;
    save_session(&s)?;
    Ok(restored)
}

pub fn runs_summary(s: &Session) -> Value {
    json!(s.runs.iter().map(|r| json!({"id": r.id, "files": r.files.keys().collect::<Vec<_>>(), "reverted": r.reverted})).collect::<Vec<_>>())
}
