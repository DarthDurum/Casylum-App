mod agent;
mod keys;
mod providers;
mod settings;
mod skills;
mod terminal;
mod tools;
mod types;

use agent::{AgentState, Decision, SendArgs};
use serde::Serialize;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::{AppHandle, State};

struct App {
    agent: Arc<AgentState>,
    http: reqwest::Client,
    terms: terminal::Terminals,
}

type Res<T> = Result<T, String>;

fn e<E: std::fmt::Display>(err: E) -> String {
    err.to_string()
}

// ---------- settings & keys ----------

#[tauri::command]
fn get_settings() -> settings::Settings {
    settings::load()
}

#[tauri::command]
fn save_settings(settings: settings::Settings) -> Res<()> {
    settings::save(&settings).map_err(e)
}

#[derive(Serialize)]
struct ProviderStatus {
    #[serde(flatten)]
    info: providers::ProviderInfo,
    has_key: bool,
    key_hint: Option<String>,
}

#[tauri::command]
async fn list_providers() -> Vec<ProviderStatus> {
    // Keychain reads can block (unlock prompts), so keep them off the main thread.
    tauri::async_runtime::spawn_blocking(|| {
        providers::catalog()
            .into_iter()
            .map(|info| {
                let hint = keys::hint(info.id);
                ProviderStatus { has_key: hint.is_some(), key_hint: hint, info }
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[tauri::command]
async fn set_api_key(provider: String, key: String) -> Res<()> {
    providers::models::invalidate(&provider);
    tauri::async_runtime::spawn_blocking(move || keys::set(&provider, &key)).await.map_err(e)?.map_err(e)
}

#[tauri::command]
async fn delete_api_key(provider: String) -> Res<()> {
    providers::models::invalidate(&provider);
    tauri::async_runtime::spawn_blocking(move || keys::delete(&provider)).await.map_err(e)?.map_err(e)
}

/// Models the user's key can use, fetched from the provider (cached).
#[tauri::command]
async fn list_models(st: State<'_, App>, provider: String, refresh: Option<bool>) -> Res<providers::models::ModelList> {
    let p = provider.clone();
    let key = tauri::async_runtime::spawn_blocking(move || keys::get(&p)).await.map_err(e)?;
    let s = settings::load();
    let base = providers::base_url(&provider, &s);
    Ok(providers::models::list(&st.http, &provider, key, base, refresh.unwrap_or(false)).await)
}

// ---------- projects & sessions ----------

#[tauri::command]
fn add_recent_project(path: String) -> Res<settings::Settings> {
    let mut s = settings::load();
    s.recent_projects.retain(|p| p != &path);
    s.recent_projects.insert(0, path);
    s.recent_projects.truncate(20);
    settings::save(&s).map_err(e)?;
    Ok(s)
}

#[tauri::command]
fn remove_recent_project(path: String) -> Res<settings::Settings> {
    let mut s = settings::load();
    s.recent_projects.retain(|p| p != &path);
    settings::save(&s).map_err(e)?;
    Ok(s)
}

#[tauri::command]
fn list_sessions(project: Option<String>) -> Vec<agent::SessionSummary> {
    agent::list_sessions(project.as_deref())
}

#[tauri::command]
fn create_session(project: String, provider: String, model: String) -> Res<agent::Session> {
    let s = agent::new_session(&project, &provider, &model);
    agent::save_session(&s).map_err(e)?;
    Ok(s)
}

#[tauri::command]
fn load_session(id: String) -> Res<Value> {
    let s = agent::load_session(&id).map_err(e)?;
    let runs = agent::runs_summary(&s);
    let mut v = serde_json::to_value(&s).map_err(e)?;
    v["runs"] = runs;
    Ok(v)
}

#[tauri::command]
fn delete_session(id: String) -> Res<()> {
    agent::delete_session(&id).map_err(e)
}

/// Each chat has its own folder; changing it never touches other chats.
#[tauri::command]
fn set_session_project(st: State<'_, App>, id: String, project: String) -> Res<()> {
    if st.agent.running.lock().contains_key(&id) {
        return Err("Wait for this chat to finish before changing its folder.".into());
    }
    let mut s = agent::load_session(&id).map_err(e)?;
    s.project = project;
    agent::save_session(&s).map_err(e)
}

#[tauri::command]
fn rename_session(id: String, title: String) -> Res<()> {
    let mut s = agent::load_session(&id).map_err(e)?;
    s.title = title;
    agent::save_session(&s).map_err(e)
}

// ---------- agent ----------

/// Replace a chat's messages after the user deletes, edits or retries one.
#[tauri::command]
fn set_messages(st: State<'_, App>, id: String, messages: Vec<types::Message>) -> Res<()> {
    if st.agent.running.lock().contains_key(&id) {
        return Err("Wait for this chat to finish first.".into());
    }
    let mut s = agent::load_session(&id).map_err(e)?;
    s.messages = messages;
    agent::save_session(&s).map_err(e)
}

#[tauri::command]
fn agent_send(
    app: AppHandle,
    st: State<'_, App>,
    session_id: String,
    text: String,
    attachments: Option<Vec<types::Block>>,
    provider: String,
    model: String,
) -> Res<()> {
    if st.agent.running.lock().contains_key(&session_id) {
        return Err("This chat is already running.".into());
    }
    let state = st.agent.clone();
    let http = st.http.clone();
    let attachments = attachments.unwrap_or_default();
    tauri::async_runtime::spawn(agent::run_agent(app, state, http, SendArgs { session_id, text, attachments, provider, model }));
    Ok(())
}

#[tauri::command]
fn agent_stop(st: State<'_, App>, session_id: String) {
    if let Some(t) = st.agent.running.lock().get(&session_id) {
        t.cancel();
    }
}

#[tauri::command]
fn agent_running(st: State<'_, App>) -> Vec<String> {
    st.agent.running.lock().keys().cloned().collect()
}

#[tauri::command]
fn agent_decide(st: State<'_, App>, request_id: String, decision: Decision) {
    if let Some(tx) = st.agent.approvals.lock().remove(&request_id) {
        let _ = tx.send(decision);
    }
}

#[tauri::command]
fn revert_run(session_id: String, run_id: String) -> Res<Vec<String>> {
    agent::revert_run(&session_id, &run_id).map_err(e)
}

// ---------- files ----------

#[derive(Serialize)]
struct Entry {
    name: String,
    path: String,
    is_dir: bool,
}

#[tauri::command]
fn fs_list(root: String, rel: Option<String>) -> Res<Vec<Entry>> {
    let rootp = std::path::Path::new(&root);
    let dir = tools::resolve(rootp, rel.as_deref().unwrap_or(".")).map_err(e)?;
    let canon_root = rootp.canonicalize().map_err(e)?;
    let mut out: Vec<Entry> = ignore::WalkBuilder::new(&dir)
        .max_depth(Some(1))
        .hidden(false)
        .filter_entry(|x| x.file_name() != ".git")
        .build()
        .flatten()
        .filter(|x| x.depth() == 1)
        .map(|x| Entry {
            name: x.file_name().to_string_lossy().to_string(),
            path: x.path().strip_prefix(&canon_root).unwrap_or(x.path()).to_string_lossy().replace('\\', "/"),
            is_dir: x.file_type().is_some_and(|t| t.is_dir()),
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

#[tauri::command]
fn fs_read(root: String, rel: String) -> Res<String> {
    let p = tools::resolve(std::path::Path::new(&root), &rel).map_err(e)?;
    let meta = std::fs::metadata(&p).map_err(e)?;
    if meta.len() > 5_000_000 {
        return Err("File is too large to preview".into());
    }
    let bytes = std::fs::read(&p).map_err(e)?;
    if bytes.iter().take(8000).any(|b| *b == 0) {
        return Err("Binary file".into());
    }
    Ok(String::from_utf8_lossy(&bytes).to_string())
}

const MAX_IMAGE: u64 = 5 * 1024 * 1024;
const MAX_TEXT: u64 = 1024 * 1024;

/// Read a file the user picked or dropped so it can ride along with a message.
#[tauri::command]
async fn read_attachment(path: String) -> Res<types::Block> {
    use base64::Engine;
    tauri::async_runtime::spawn_blocking(move || {
        let p = std::path::Path::new(&path);
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.clone());
        let meta = std::fs::metadata(p).map_err(e)?;
        if meta.is_dir() {
            return Err(format!("{name} is a folder"));
        }
        let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        let image = match ext.as_str() {
            "png" => Some("image/png"),
            "jpg" | "jpeg" => Some("image/jpeg"),
            "gif" => Some("image/gif"),
            "webp" => Some("image/webp"),
            _ => None,
        };
        if let Some(media_type) = image {
            if meta.len() > MAX_IMAGE {
                return Err(format!("{name} is over 5 MB"));
            }
            let data = base64::engine::general_purpose::STANDARD.encode(std::fs::read(p).map_err(e)?);
            return Ok(types::Block::Image { name, media_type: media_type.into(), data });
        }
        if meta.len() > MAX_TEXT {
            return Err(format!("{name} is over 1 MB"));
        }
        let bytes = std::fs::read(p).map_err(e)?;
        if bytes.iter().take(8000).any(|b| *b == 0) {
            return Err(format!("{name} isn't a text file or image"));
        }
        Ok(types::Block::File { name, text: String::from_utf8_lossy(&bytes).to_string() })
    })
    .await
    .map_err(e)?
}

// ---------- skills ----------

#[tauri::command]
fn skills_list(project: Option<String>) -> Vec<skills::Skill> {
    skills::list(project.as_deref(), &settings::load().disabled_skills)
}

#[tauri::command]
fn skill_read(dir: String) -> Res<String> {
    skills::read(&dir).map_err(e)
}

#[tauri::command]
fn skill_save(scope: String, project: Option<String>, name: String, content: String) -> Res<String> {
    skills::save(&scope, project.as_deref(), &name, &content).map_err(e)
}

#[tauri::command]
fn skill_delete(dir: String) -> Res<()> {
    skills::delete(&dir).map_err(e)
}

#[tauri::command]
fn skill_toggle(scope: String, name: String, enabled: bool) -> Res<()> {
    let mut s = settings::load();
    let key = format!("{scope}:{name}");
    s.disabled_skills.retain(|k| k != &key);
    if !enabled {
        s.disabled_skills.push(key);
    }
    settings::save(&s).map_err(e)
}

#[tauri::command]
fn skills_paths(project: Option<String>) -> Value {
    json!({
        "global": skills::global_dir().to_string_lossy(),
        "project": project.filter(|p| !p.is_empty()).map(|p| skills::project_dir(&p).to_string_lossy().to_string()),
    })
}

// ---------- terminal ----------

#[tauri::command]
fn pty_spawn(app: AppHandle, st: State<'_, App>, cwd: String, cols: u16, rows: u16) -> Res<String> {
    st.terms.spawn(app, &cwd, cols, rows).map_err(e)
}

#[tauri::command]
fn pty_write(st: State<'_, App>, id: String, data: String) -> Res<()> {
    st.terms.write(&id, &data).map_err(e)
}

#[tauri::command]
fn pty_resize(st: State<'_, App>, id: String, cols: u16, rows: u16) -> Res<()> {
    st.terms.resize(&id, cols, rows).map_err(e)
}

#[tauri::command]
fn pty_kill(st: State<'_, App>, id: String) {
    st.terms.kill(&id);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(20))
        .user_agent(concat!("Casylum/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // In-app updates come from GitHub Releases (see plugins.updater in tauri.conf.json).
            #[cfg(desktop)]
            {
                app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
                app.handle().plugin(tauri_plugin_process::init())?;
            }
            Ok(())
        })
        .manage(App { agent: Arc::new(AgentState::default()), http, terms: Default::default() })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            list_providers,
            set_api_key,
            delete_api_key,
            list_models,
            add_recent_project,
            remove_recent_project,
            list_sessions,
            create_session,
            load_session,
            delete_session,
            rename_session,
            set_messages,
            set_session_project,
            read_attachment,
            agent_send,
            agent_stop,
            agent_running,
            agent_decide,
            revert_run,
            fs_list,
            fs_read,
            skills_list,
            skill_read,
            skill_save,
            skill_delete,
            skill_toggle,
            skills_paths,
            pty_spawn,
            pty_write,
            pty_resize,
            pty_kill,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Casylum");
}
