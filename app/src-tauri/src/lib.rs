//! Thin Tauri shell over `zn_core`. Commands stay small: load settings,
//! call into the core crate, serialize the result. See
//! docs/ARCHITECTURE.md for the command table.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};

use zn_core::llm::{AnyProvider, SummarizeRequest};
use zn_core::llm::provider::{ANTHROPIC_BASE_URL, GEMINI_BASE_URL};
use zn_core::models::{
    AppSettings, AuditProposal, ChatDelta, ChatMessage, ClassificationDecision,
    ClassificationProposal, Item, Library, MoveResult, ProgressEvent, ProviderId, StoredSummary,
    SummarySource, ZoteroStatus, UNCLASSIFIED_COLLECTION,
};
use zn_core::zotero::{local_api, plugin_api};
use zn_core::{classify, db, keychain, settings, Error, Result};

/// Cap on how much extracted PDF text is sent to an LLM (full-text summary
/// and chat). ~80k chars ≈ 20k tokens.
const BODY_MAX_CHARS: usize = 80_000;

struct AppState {
    db: Mutex<db::Db>,
    config_dir: PathBuf,
}

impl AppState {
    fn settings_path(&self) -> PathBuf {
        self.config_dir.join("settings.json")
    }

    fn settings(&self) -> AppSettings {
        settings::load(&self.settings_path())
    }
}

/// Fetch a keychain entry off the async runtime. On Linux the keyring
/// backend blocks on its own internal runtime — calling it directly from a
/// tokio worker thread can panic ("cannot start a runtime from within a
/// runtime"), so every keychain access from an async command goes through
/// spawn_blocking.
async fn get_key_blocking(id: ProviderId) -> Result<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || keychain::get_key(id))
        .await
        .map_err(|e| Error::Other(format!("keychain task failed: {e}")))?
}

async fn build_provider(provider: Option<ProviderId>, s: &AppSettings) -> Result<AnyProvider> {
    let id = provider.unwrap_or(s.default_provider);
    // Local servers don't require a key; a stored one is forwarded as a
    // Bearer token for setups that do (e.g. a shared llama.cpp box).
    if id == ProviderId::Local {
        let key = get_key_blocking(id).await.unwrap_or(None);
        return Ok(AnyProvider::Local(
            zn_core::llm::openai_compat::OpenAiCompatClient::new(
                key,
                s.local_model.clone(),
                s.local_base_url.clone(),
            ),
        ));
    }
    let key = get_key_blocking(id)
        .await?
        .ok_or_else(|| Error::MissingApiKey(id.as_str().to_string()))?;
    Ok(match id {
        ProviderId::Gemini => AnyProvider::Gemini(zn_core::llm::gemini::GeminiClient::new(
            key,
            s.gemini_model.clone(),
            GEMINI_BASE_URL.to_string(),
        )),
        ProviderId::Anthropic => {
            AnyProvider::Anthropic(zn_core::llm::anthropic::AnthropicClient::new(
                key,
                s.anthropic_model.clone(),
                ANTHROPIC_BASE_URL.to_string(),
            ))
        }
        ProviderId::Local => unreachable!("handled above"),
    })
}

/// Plugin library when available, read-only local API otherwise.
async fn fetch_library_any(s: &AppSettings) -> Result<Library> {
    let client = plugin_api::PluginClient::new(&s.zotero_base_url);
    match client.fetch_library().await {
        Ok(lib) => Ok(lib),
        Err(Error::PluginMissing) => local_api::fetch_library(&s.zotero_base_url).await,
        Err(e) => Err(e),
    }
}

/// Fill a missing abstract from public metadata APIs (Crossref → Semantic
/// Scholar → OpenAlex; best-effort, never errors). Returns whether the item
/// has an abstract afterwards — the flag stored on summaries and surfaced
/// as a "metadata only" badge in the UI.
async fn ensure_abstract(item: &mut Item) -> bool {
    if item
        .abstract_text
        .as_deref()
        .is_some_and(|a| !a.trim().is_empty())
    {
        return true;
    }
    let sources = zn_core::abstract_lookup::Sources::default();
    match zn_core::abstract_lookup::lookup(&sources, item.doi.as_deref(), &item.title).await {
        Some(found) => {
            log::info!("fetched missing abstract for {}", item.key);
            item.abstract_text = Some(found);
            true
        }
        None => false,
    }
}

fn find_item(library: &Library, item_key: &str) -> Result<Item> {
    library
        .items
        .iter()
        .find(|i| i.key == item_key)
        .cloned()
        .ok_or_else(|| Error::Other(format!("item {item_key} not found in the library")))
}

fn emit_progress(app: &AppHandle, event: &str, p: ProgressEvent) {
    if let Err(e) = app.emit(event, &p) {
        log::warn!("failed to emit {event}: {e}");
    }
}

// --- commands ---------------------------------------------------------

#[tauri::command]
async fn get_status(state: State<'_, AppState>) -> Result<ZoteroStatus> {
    let s = state.settings();
    Ok(plugin_api::check_status(&s.zotero_base_url).await)
}

#[tauri::command]
async fn get_library(state: State<'_, AppState>) -> Result<Library> {
    let s = state.settings();
    fetch_library_any(&s).await
}

#[tauri::command]
fn get_summary(state: State<'_, AppState>, item_key: String) -> Result<Option<StoredSummary>> {
    let db = state.db.lock().expect("db mutex");
    db.get_summary(&item_key)
}

/// Fetch the paper's extracted text through the plugin (best-effort).
async fn fetch_body_excerpt(s: &AppSettings, item_key: &str) -> Option<String> {
    let client = plugin_api::PluginClient::new(&s.zotero_base_url);
    match client.fetch_fulltext(item_key, BODY_MAX_CHARS).await {
        Ok(Some(ft)) => Some(ft.text),
        Ok(None) => None,
        Err(e) => {
            log::warn!("fulltext fetch for {item_key} failed: {e}");
            None
        }
    }
}

#[tauri::command]
async fn summarize_item(
    state: State<'_, AppState>,
    item_key: String,
    provider: Option<ProviderId>,
    use_fulltext: Option<bool>,
) -> Result<StoredSummary> {
    let s = state.settings();
    let library = fetch_library_any(&s).await?;
    let mut item = find_item(&library, &item_key)?;
    let llm = build_provider(provider, &s).await?;

    // The cheap metadata+abstract summary is the default; reading the whole
    // PDF is an explicit, separate action (it costs real tokens).
    let body_excerpt = if use_fulltext.unwrap_or(false) {
        fetch_body_excerpt(&s, &item_key).await
    } else {
        None
    };
    let has_abstract = ensure_abstract(&mut item).await;
    let source = if body_excerpt.is_some() {
        SummarySource::Fulltext
    } else if has_abstract {
        SummarySource::Abstract
    } else {
        SummarySource::Metadata
    };

    let req = SummarizeRequest {
        title: item.title.clone(),
        creators: item.creators.clone(),
        year: item.year,
        publication: item.publication.clone(),
        abstract_text: item.abstract_text.clone(),
        body_excerpt,
    };
    let text = llm.summarize(&req).await?;
    let model = match llm.id() {
        ProviderId::Gemini => s.gemini_model.clone(),
        ProviderId::Anthropic => s.anthropic_model.clone(),
        ProviderId::Local => s.local_model.clone(),
    };
    let summary = StoredSummary {
        item_key: item_key.clone(),
        summary: text,
        provider: llm.id().as_str().to_string(),
        model,
        created_at: chrono::Utc::now().to_rfc3339(),
        source,
    };
    {
        let db = state.db.lock().expect("db mutex");
        db.upsert_summary(&summary)?;
    }
    Ok(summary)
}

/// One turn of the per-paper "Ask AI" chat. `history` must end with the
/// user's newest question. Streams text fragments as `chat-delta` events
/// and resolves with the complete answer.
#[tauri::command]
async fn chat_with_item(
    app: AppHandle,
    state: State<'_, AppState>,
    item_key: String,
    history: Vec<ChatMessage>,
    provider: Option<ProviderId>,
) -> Result<String> {
    if history.is_empty() {
        return Err(Error::Other("the conversation is empty".into()));
    }
    let s = state.settings();
    let library = fetch_library_any(&s).await?;
    let mut item = find_item(&library, &item_key)?;
    let llm = build_provider(provider, &s).await?;

    let body_excerpt = fetch_body_excerpt(&s, &item_key).await;
    ensure_abstract(&mut item).await;
    let system = zn_core::llm::provider::chat_system_prompt(
        &item.title,
        &item.creators,
        item.year,
        item.publication.as_deref(),
        item.abstract_text.as_deref(),
        body_excerpt.as_deref(),
    );

    let mut on_delta = |t: &str| {
        if let Err(e) = app.emit(
            "chat-delta",
            &ChatDelta {
                item_key: item_key.clone(),
                delta: t.to_string(),
            },
        ) {
            log::warn!("failed to emit chat-delta: {e}");
        }
    };
    llm.chat_stream(&system, &history, &mut on_delta).await
}

#[tauri::command]
async fn classify_items(
    app: AppHandle,
    state: State<'_, AppState>,
    item_keys: Vec<String>,
    provider: Option<ProviderId>,
) -> Result<Vec<ClassificationProposal>> {
    let s = state.settings();
    let library = fetch_library_any(&s).await?;
    let llm = build_provider(provider, &s).await?;
    let total = item_keys.len();
    let mut proposals = Vec::new();

    for (i, key) in item_keys.iter().enumerate() {
        let mut item = match find_item(&library, key) {
            Ok(it) => it,
            Err(e) => {
                emit_progress(
                    &app,
                    "classify-progress",
                    ProgressEvent {
                        done: i + 1,
                        total,
                        item_key: Some(key.clone()),
                        state: "error".into(),
                        message: Some(e.to_string()),
                    },
                );
                continue;
            }
        };
        emit_progress(
            &app,
            "classify-progress",
            ProgressEvent {
                done: i,
                total,
                item_key: Some(key.clone()),
                state: "running".into(),
                message: Some(item.title.clone()),
            },
        );
        let outcome = async {
            ensure_abstract(&mut item).await;
            let req = classify::build_request(&item, &library);
            let resp = llm.classify(&req).await?;
            classify::to_proposal(key, resp, &library)
        }
        .await;
        match outcome {
            Ok(p) => {
                proposals.push(p);
                emit_progress(
                    &app,
                    "classify-progress",
                    ProgressEvent {
                        done: i + 1,
                        total,
                        item_key: Some(key.clone()),
                        state: "ok".into(),
                        message: None,
                    },
                );
            }
            Err(e) => emit_progress(
                &app,
                "classify-progress",
                ProgressEvent {
                    done: i + 1,
                    total,
                    item_key: Some(key.clone()),
                    state: "error".into(),
                    message: Some(e.to_string()),
                },
            ),
        }
    }
    Ok(proposals)
}

/// Audit already-classified papers: for each item, ask the LLM whether its
/// current filing fits and collect refile proposals. Items the model judges
/// correctly filed yield no proposal (progress event message "ok").
#[tauri::command]
async fn audit_items(
    app: AppHandle,
    state: State<'_, AppState>,
    item_keys: Vec<String>,
    provider: Option<ProviderId>,
) -> Result<Vec<AuditProposal>> {
    let s = state.settings();
    let library = fetch_library_any(&s).await?;
    let llm = build_provider(provider, &s).await?;
    let total = item_keys.len();
    let mut proposals = Vec::new();

    for (i, key) in item_keys.iter().enumerate() {
        let emit = |done: usize, st: &str, message: Option<String>| {
            emit_progress(
                &app,
                "audit-progress",
                ProgressEvent {
                    done,
                    total,
                    item_key: Some(key.clone()),
                    state: st.into(),
                    message,
                },
            );
        };
        let mut item = match find_item(&library, key) {
            Ok(it) => it,
            Err(e) => {
                emit(i + 1, "error", Some(e.to_string()));
                continue;
            }
        };
        emit(i, "running", Some(item.title.clone()));
        let outcome = async {
            ensure_abstract(&mut item).await;
            let req = classify::build_audit_request(&item, &library)
                .ok_or_else(|| Error::Other("not classified yet — use the Unclassified flow".into()))?;
            let resp = llm.audit(&req).await?;
            classify::audit_to_proposal(&item, resp, &library)
        }
        .await;
        match outcome {
            Ok(Some(p)) => {
                proposals.push(p);
                emit(i + 1, "ok", Some("misplaced".into()));
            }
            Ok(None) => emit(i + 1, "ok", None),
            Err(e) => emit(i + 1, "error", Some(e.to_string())),
        }
    }
    Ok(proposals)
}

#[tauri::command]
async fn apply_classifications(
    app: AppHandle,
    state: State<'_, AppState>,
    decisions: Vec<ClassificationDecision>,
) -> Result<Vec<MoveResult>> {
    let s = state.settings();
    let client = plugin_api::PluginClient::new(&s.zotero_base_url);

    // Writes are gated on a compatible plugin (docs/PLUGIN_API.md,
    // "Versioning"): an older plugin could silently mis-handle the wire
    // format, so refuse instead.
    let (plugin_version, _) = client.ping().await?;
    if !plugin_api::plugin_version_compatible(&plugin_version) {
        return Err(Error::Other(format!(
            "the Zotero Notebook plugin (v{plugin_version}) is older than this app expects — \
             update it from Settings → Zotero before applying moves"
        )));
    }

    let library = client.fetch_library().await?; // moves require the plugin
    let unclassified_key: Vec<String> = library
        .collections
        .iter()
        .filter(|c| {
            c.parent_key.is_none()
                && c.name.trim().eq_ignore_ascii_case(UNCLASSIFIED_COLLECTION)
        })
        .map(|c| c.key.clone())
        .collect();

    let total = decisions.len();
    let mut results = Vec::with_capacity(total);
    for (i, d) in decisions.iter().enumerate() {
        emit_progress(
            &app,
            "apply-progress",
            ProgressEvent {
                done: i,
                total,
                item_key: Some(d.item_key.clone()),
                state: "running".into(),
                message: None,
            },
        );
        // Always drop the Unclassified membership; the audit flow also
        // drops the memberships it judged wrong (decision.remove_collection_keys).
        let mut remove_from = unclassified_key.clone();
        for key in &d.remove_collection_keys {
            if !remove_from.contains(key) {
                remove_from.push(key.clone());
            }
        }
        let result = client
            .move_item(
                &d.item_key,
                &d.target_path,
                &remove_from,
                s.file_root.as_deref(),
            )
            .await
            .unwrap_or_else(|e| MoveResult {
                item_key: d.item_key.clone(),
                ok: false,
                error: Some(e.to_string()),
                collection_key: None,
                new_file_path: None,
            });
        emit_progress(
            &app,
            "apply-progress",
            ProgressEvent {
                done: i + 1,
                total,
                item_key: Some(d.item_key.clone()),
                state: if result.ok { "ok" } else { "error" }.into(),
                message: result.error.clone(),
            },
        );
        results.push(result);
    }
    Ok(results)
}

#[tauri::command]
async fn reveal_item_file(
    app: AppHandle,
    state: State<'_, AppState>,
    item_key: String,
) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let s = state.settings();
    let library = fetch_library_any(&s).await?;
    let item = find_item(&library, &item_key)?;
    let path = item
        .attachment
        .and_then(|a| a.file_path)
        .ok_or_else(|| Error::Other("this item has no PDF file on disk".to_string()))?;
    if !Path::new(&path).exists() {
        return Err(Error::Other(format!("file not found on disk: {path}")));
    }
    // Opens the OS file manager with the file selected (Explorer/Finder/...).
    app.opener()
        .reveal_item_in_dir(&path)
        .map_err(|e| Error::Other(format!("failed to open the file manager: {e}")))
}

#[tauri::command]
async fn open_item_pdf(app: AppHandle, state: State<'_, AppState>, item_key: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let s = state.settings();
    let library = fetch_library_any(&s).await?;
    let item = find_item(&library, &item_key)?;
    let path = item
        .attachment
        .and_then(|a| a.file_path)
        .ok_or_else(|| Error::Other("this item has no PDF file on disk".to_string()))?;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| Error::Other(format!("failed to open the PDF: {e}")))
}

#[tauri::command]
fn open_in_zotero(app: AppHandle, item_key: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let url = format!("zotero://select/library/items/{item_key}");
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| Error::Other(format!("failed to open Zotero: {e}")))
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<AppSettings> {
    Ok(state.settings())
}

#[tauri::command]
fn save_settings(state: State<'_, AppState>, settings: AppSettings) -> Result<()> {
    zn_core::settings::save(&state.settings_path(), &settings)
}

fn keychain_task<T: Send + 'static>(
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> tauri::async_runtime::JoinHandle<Result<T>> {
    tauri::async_runtime::spawn_blocking(f)
}

#[tauri::command]
async fn save_api_key(provider: ProviderId, key: String) -> Result<()> {
    keychain_task(move || keychain::save_key(provider, &key))
        .await
        .map_err(|e| Error::Other(format!("keychain task failed: {e}")))?
}

#[tauri::command]
async fn has_api_key(provider: ProviderId) -> Result<bool> {
    keychain_task(move || keychain::has_key(provider))
        .await
        .map_err(|e| Error::Other(format!("keychain task failed: {e}")))?
}

#[tauri::command]
async fn delete_api_key(provider: ProviderId) -> Result<()> {
    keychain_task(move || keychain::delete_key(provider))
        .await
        .map_err(|e| Error::Other(format!("keychain task failed: {e}")))?
}

#[tauri::command]
async fn test_api_key(state: State<'_, AppState>, provider: ProviderId) -> Result<()> {
    let s = state.settings();
    let llm = build_provider(Some(provider), &s).await?;
    llm.test_key().await
}

#[tauri::command]
fn export_plugin_xpi(app: AppHandle, dest_dir: String) -> Result<String> {
    let resource = app
        .path()
        .resolve(
            "resources/zotero-notebook.xpi",
            tauri::path::BaseDirectory::Resource,
        )
        .map_err(|e| Error::Other(format!("bundled plugin not found: {e}")))?;
    let dest = PathBuf::from(&dest_dir).join("zotero-notebook.xpi");
    std::fs::copy(&resource, &dest)
        .map_err(|e| Error::Other(format!("failed to write the plugin file: {e}")))?;
    Ok(dest.to_string_lossy().into_owned())
}

// --- app entry --------------------------------------------------------

fn spawn_status_watcher(app: AppHandle, config_dir: PathBuf) {
    tauri::async_runtime::spawn(async move {
        loop {
            let s = settings::load(&config_dir.join("settings.json"));
            let status = plugin_api::check_status(&s.zotero_base_url).await;
            if let Err(e) = app.emit("zotero-status", &status) {
                log::warn!("failed to emit zotero-status: {e}");
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Webview),
                ])
                .build(),
        )
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let config_dir = app
                .path()
                .app_config_dir()
                .expect("app config dir must resolve");
            let database = db::Db::open(&config_dir.join("summaries.sqlite")).unwrap_or_else(|e| {
                log::error!("could not open the summaries database ({e}); using in-memory");
                db::Db::open_in_memory().expect("in-memory sqlite")
            });
            app.manage(AppState {
                db: Mutex::new(database),
                config_dir: config_dir.clone(),
            });
            spawn_status_watcher(app.handle().clone(), config_dir);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_library,
            get_summary,
            summarize_item,
            chat_with_item,
            classify_items,
            audit_items,
            apply_classifications,
            reveal_item_file,
            open_item_pdf,
            open_in_zotero,
            get_settings,
            save_settings,
            save_api_key,
            has_api_key,
            delete_api_key,
            test_api_key,
            export_plugin_xpi
        ])
        .run(tauri::generate_context!())
        .expect("error while running Zotero Notebook");
}
