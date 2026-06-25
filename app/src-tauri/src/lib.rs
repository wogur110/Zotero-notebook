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
    AppSettings, AuditProposal, ChatDelta, ChatMessage, CitationGraph, ClassificationDecision,
    ClassificationProposal, Item, Library, MoveResult, ProgressEvent, ProviderId, ReadingState,
    ReadingStatus, StoredSummary, SummarySource, SynthesisDelta, UsageSummary, ZoteroStatus,
    UNCLASSIFIED_COLLECTION,
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

/// `ensure_abstract` plus write-back: when an abstract was fetched online
/// and the setting is on, fill the Zotero item's empty abstract field via
/// the plugin (best-effort — the plugin never overwrites existing data).
async fn ensure_abstract_synced(s: &AppSettings, item: &mut Item) -> bool {
    let already_had = item
        .abstract_text
        .as_deref()
        .is_some_and(|a| !a.trim().is_empty());
    let has = ensure_abstract(item).await;
    if has && !already_had && s.write_back_abstracts {
        let client = plugin_api::PluginClient::new(&s.zotero_base_url);
        match client
            .update_item(&item.key, item.abstract_text.as_deref(), &[], None)
            .await
        {
            Ok(r) if r.wrote_abstract => {
                log::info!("wrote fetched abstract back to Zotero item {}", item.key)
            }
            Ok(_) => {}
            Err(e) => log::warn!("abstract write-back for {} failed: {e}", item.key),
        }
    }
    has
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

fn model_name(llm: &AnyProvider, s: &AppSettings) -> String {
    match llm.id() {
        ProviderId::Gemini => s.gemini_model.clone(),
        ProviderId::Anthropic => s.anthropic_model.clone(),
        ProviderId::Local => s.local_model.clone(),
    }
}

/// Record the token usage of the provider's most recent call into the usage
/// ledger (best-effort; cloud cost is an estimate, local is free). Read the
/// side-channel right after the `summarize`/`classify`/`audit` call.
fn record_usage(state: &State<'_, AppState>, llm: &AnyProvider, s: &AppSettings, op: &str) {
    let Some(u) = llm.last_usage() else { return };
    if u.is_empty() {
        return;
    }
    let model = model_name(llm, s);
    let cost = zn_core::pricing::cost_usd(llm.id(), &model, u.input_tokens, u.output_tokens);
    let db = state.db.lock().expect("db mutex");
    if let Err(e) = db.insert_usage(
        op,
        llm.id().as_str(),
        &model,
        u.input_tokens,
        u.output_tokens,
        cost,
        &chrono::Utc::now().to_rfc3339(),
    ) {
        log::warn!("failed to record usage: {e}");
    }
}

/// Emit the current cumulative usage totals so the UI cost indicator updates.
fn emit_usage(app: &AppHandle, state: &State<'_, AppState>) {
    let summary = {
        let db = state.db.lock().expect("db mutex");
        db.usage_summary().ok()
    };
    if let Some(summary) = summary {
        if let Err(e) = app.emit("usage-update", &summary) {
            log::warn!("failed to emit usage-update: {e}");
        }
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

/// Summarize one item and persist the result. Shared by the single-item
/// command and the batch loop. The cheap metadata+abstract summary is the
/// default; reading the whole PDF is an explicit, separate action.
async fn do_summarize(
    state: &State<'_, AppState>,
    s: &AppSettings,
    llm: &AnyProvider,
    library: &Library,
    item_key: &str,
    use_fulltext: bool,
) -> Result<StoredSummary> {
    let mut item = find_item(library, item_key)?;
    let body_excerpt = if use_fulltext {
        fetch_body_excerpt(s, item_key).await
    } else {
        None
    };
    let has_abstract = ensure_abstract_synced(s, &mut item).await;
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
        language: s.output_language.clone(),
    };
    let text = llm.summarize(&req).await?;
    record_usage(state, llm, s, "summary");
    let model = model_name(llm, s);
    let summary = StoredSummary {
        item_key: item_key.to_string(),
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
    if s.sync_summary_notes {
        let client = plugin_api::PluginClient::new(&s.zotero_base_url);
        if let Err(e) = client
            .update_item(item_key, None, &[], Some(&summary.note_html()))
            .await
        {
            // Best-effort: a missing/old plugin must not fail summarization.
            log::warn!("summary-note sync for {item_key} failed: {e}");
        }
    }
    Ok(summary)
}

/// Manual "Save to Zotero" for the stored summary (the automatic sync can
/// be off, or the plugin may have been unavailable when it ran).
#[tauri::command]
async fn save_summary_note(state: State<'_, AppState>, item_key: String) -> Result<()> {
    let s = state.settings();
    let summary = {
        let db = state.db.lock().expect("db mutex");
        db.get_summary(&item_key)?
    }
    .ok_or_else(|| Error::Other("no summary to save — generate one first".into()))?;
    let client = plugin_api::PluginClient::new(&s.zotero_base_url);
    client
        .update_item(&item_key, None, &[], Some(&summary.note_html()))
        .await?;
    Ok(())
}

#[tauri::command]
async fn summarize_item(
    app: AppHandle,
    state: State<'_, AppState>,
    item_key: String,
    provider: Option<ProviderId>,
    use_fulltext: Option<bool>,
) -> Result<StoredSummary> {
    let s = state.settings();
    let library = fetch_library_any(&s).await?;
    let llm = build_provider(provider, &s).await?;
    let result = do_summarize(
        &state,
        &s,
        &llm,
        &library,
        &item_key,
        use_fulltext.unwrap_or(false),
    )
    .await;
    emit_usage(&app, &state);
    result
}

/// Batch-summarize (quick mode: metadata + abstract). Sequential, emits
/// `summarize-progress` per item, never aborts the batch on per-item
/// failures; returns the successfully created summaries.
#[tauri::command]
async fn summarize_items(
    app: AppHandle,
    state: State<'_, AppState>,
    item_keys: Vec<String>,
    provider: Option<ProviderId>,
) -> Result<Vec<StoredSummary>> {
    let s = state.settings();
    let library = fetch_library_any(&s).await?;
    let llm = build_provider(provider, &s).await?;
    let total = item_keys.len();
    let mut done = Vec::new();

    for (i, key) in item_keys.iter().enumerate() {
        let title = find_item(&library, key).map(|it| it.title).ok();
        emit_progress(
            &app,
            "summarize-progress",
            ProgressEvent {
                done: i,
                total,
                item_key: Some(key.clone()),
                state: "running".into(),
                message: title,
            },
        );
        match do_summarize(&state, &s, &llm, &library, key, false).await {
            Ok(summary) => {
                done.push(summary);
                emit_progress(
                    &app,
                    "summarize-progress",
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
                "summarize-progress",
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
    emit_usage(&app, &state);
    Ok(done)
}

#[tauri::command]
fn get_all_summaries(state: State<'_, AppState>) -> Result<Vec<StoredSummary>> {
    let db = state.db.lock().expect("db mutex");
    db.all_summaries()
}

/// Cumulative AI token/cost totals (also pushed live via `usage-update`).
#[tauri::command]
fn get_usage_summary(state: State<'_, AppState>) -> Result<UsageSummary> {
    let db = state.db.lock().expect("db mutex");
    db.usage_summary()
}

/// The whole reading queue (every tracked item's status/star/note).
#[tauri::command]
fn get_reading_states(state: State<'_, AppState>) -> Result<Vec<ReadingState>> {
    let db = state.db.lock().expect("db mutex");
    db.all_reading_states()
}

/// How long a cached citation graph stays fresh. References are stable and
/// citations grow slowly, so a long TTL is fine; a manual refresh bypasses it.
const CITATION_CACHE_TTL_DAYS: i64 = 14;

fn citation_cache_stale(fetched_at: &str) -> bool {
    match chrono::DateTime::parse_from_rfc3339(fetched_at) {
        Ok(t) => {
            chrono::Utc::now()
                .signed_duration_since(t.with_timezone(&chrono::Utc))
                .num_days()
                >= CITATION_CACHE_TTL_DAYS
        }
        Err(_) => true,
    }
}

/// The citation graph (references + citing works) for one item, each entry
/// tagged with library membership. Read-only / suggest-only: nothing is
/// written to Zotero. Cached in the sidecar (14-day TTL); `refresh` bypasses
/// the cache. `fetchFailed` is set when the item has no DOI or OpenAlex could
/// not be reached.
#[tauri::command]
async fn fetch_citation_graph(
    state: State<'_, AppState>,
    item_key: String,
    refresh: Option<bool>,
) -> Result<CitationGraph> {
    let s = state.settings();
    let library = fetch_library_any(&s).await?;
    let item = find_item(&library, &item_key)?;
    let doi = item.doi.as_deref().map(str::trim).filter(|d| !d.is_empty());

    let cached = {
        let db = state.db.lock().expect("db mutex");
        db.get_citation_cache(&item_key)?
    };

    // Use the cache unless a refresh was requested or it's stale.
    let mut graph: Option<CitationGraph> = if refresh.unwrap_or(false) {
        None
    } else {
        cached
            .filter(|(_, at)| !citation_cache_stale(at))
            .and_then(|(json, _)| zn_core::citations::from_cache_json(&json))
    };

    if graph.is_none() {
        let Some(doi) = doi else {
            return Ok(CitationGraph {
                fetch_failed: true,
                ..Default::default()
            });
        };
        let openalex = zn_core::abstract_lookup::Sources::default().openalex;
        match zn_core::citations::fetch(&openalex, doi).await {
            Some(g) => {
                let json = zn_core::citations::to_cache_json(&g);
                if !json.is_empty() {
                    let db = state.db.lock().expect("db mutex");
                    let _ = db.upsert_citation_cache(
                        &item_key,
                        &json,
                        &chrono::Utc::now().to_rfc3339(),
                    );
                }
                graph = Some(g);
            }
            None => {
                return Ok(CitationGraph {
                    fetch_failed: true,
                    ..Default::default()
                })
            }
        }
    }

    let mut graph = graph.expect("graph is Some here");
    zn_core::citations::apply_library_match(&mut graph, &library);
    Ok(graph)
}

/// Set (or clear) one item's reading state. Status, star, and note are
/// independent: a paper can be starred with no reading status (starring never
/// forces a status). When all three are empty the row is deleted (untracked).
/// Returns the resulting state, or null when cleared.
#[tauri::command]
fn set_reading_state(
    state: State<'_, AppState>,
    item_key: String,
    status: Option<ReadingStatus>,
    starred: bool,
    note: String,
) -> Result<Option<ReadingState>> {
    let db = state.db.lock().expect("db mutex");
    let note = note.trim().to_string();
    if status.is_none() && !starred && note.is_empty() {
        db.delete_reading_state(&item_key)?;
        return Ok(None);
    }
    let s = ReadingState {
        item_key,
        status,
        starred,
        note,
        updated_at: chrono::Utc::now().to_rfc3339(),
    };
    db.upsert_reading_state(&s)?;
    Ok(Some(s))
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
    ensure_abstract_synced(&s, &mut item).await;
    let system = zn_core::llm::provider::chat_system_prompt(
        &item.title,
        &item.creators,
        item.year,
        item.publication.as_deref(),
        item.abstract_text.as_deref(),
        body_excerpt.as_deref(),
        &s.output_language,
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

/// One turn of multi-paper synthesis / Q&A over a set of items — a whole
/// collection or an ad-hoc selection. Context is metadata + abstracts only
/// (no PDF text), capped at `MAX_SYNTHESIS_PAPERS`. Streams fragments as
/// `synthesis-delta` events and resolves with the complete answer.
#[tauri::command]
async fn chat_with_items(
    app: AppHandle,
    state: State<'_, AppState>,
    item_keys: Vec<String>,
    history: Vec<ChatMessage>,
    provider: Option<ProviderId>,
) -> Result<String> {
    if history.is_empty() {
        return Err(Error::Other("the conversation is empty".into()));
    }
    if item_keys.is_empty() {
        return Err(Error::Other("no papers selected".into()));
    }
    let s = state.settings();
    let library = fetch_library_any(&s).await?;
    // Resolve keys to items, preserving the requested order; silently skip any
    // that vanished from the library since the UI loaded it.
    let items: Vec<Item> = item_keys
        .iter()
        .filter_map(|k| library.items.iter().find(|i| &i.key == k).cloned())
        .collect();
    if items.is_empty() {
        return Err(Error::Other(
            "none of the selected papers are in the library".into(),
        ));
    }
    let ctx = zn_core::synthesis::build_context(&items);
    let llm = build_provider(provider, &s).await?;
    let system = zn_core::llm::provider::synthesis_system_prompt(&ctx.papers, &s.output_language);

    let mut on_delta = |t: &str| {
        if let Err(e) = app.emit(
            "synthesis-delta",
            &SynthesisDelta {
                delta: t.to_string(),
            },
        ) {
            log::warn!("failed to emit synthesis-delta: {e}");
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
            ensure_abstract_synced(&s, &mut item).await;
            let mut req = classify::build_request(&item, &library);
            req.language = s.output_language.clone();
            let resp = llm.classify(&req).await?;
            classify::to_proposal(&item, resp, &library)
        }
        .await;
        if outcome.is_ok() {
            record_usage(&state, &llm, &s, "classify");
        }
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
    emit_usage(&app, &state);
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
            ensure_abstract_synced(&s, &mut item).await;
            let mut req = classify::build_audit_request(&item, &library)
                .ok_or_else(|| Error::Other("not classified yet — use the Unclassified flow".into()))?;
            req.language = s.output_language.clone();
            let resp = llm.audit(&req).await?;
            classify::audit_to_proposal(&item, resp, &library)
        }
        .await;
        if outcome.is_ok() {
            record_usage(&state, &llm, &s, "filing-check");
        }
        match outcome {
            Ok(Some(p)) => {
                proposals.push(p);
                emit(i + 1, "ok", Some("misplaced".into()));
            }
            Ok(None) => emit(i + 1, "ok", None),
            Err(e) => emit(i + 1, "error", Some(e.to_string())),
        }
    }
    emit_usage(&app, &state);
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
        // Approved AI tags ride along with a successful move (additive;
        // failures here are secondary and must not fail the move).
        if result.ok && !d.add_tags.is_empty() {
            if let Err(e) = client
                .update_item(&d.item_key, None, &d.add_tags, None)
                .await
            {
                log::warn!("tag write-back for {} failed: {e}", d.item_key);
            }
        }
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

/// Write UTF-8 text to a path the user chose via the save dialog. Backs the
/// "Export to Markdown" actions (review document / annotated bibliography).
#[tauri::command]
fn write_text_file(path: String, content: String) -> Result<()> {
    std::fs::write(&path, content).map_err(|e| Error::Other(format!("failed to write {path}: {e}")))
}

/// Add tags to multiple items (additive; existing tags are kept, never
/// removed). Backs the bulk "Add tag" action. Returns per-item results so the
/// UI can report partial failures; needs the companion plugin.
#[tauri::command]
async fn add_tags(
    state: State<'_, AppState>,
    item_keys: Vec<String>,
    tags: Vec<String>,
) -> Result<Vec<MoveResult>> {
    let s = state.settings();
    let client = plugin_api::PluginClient::new(&s.zotero_base_url);
    let mut results = Vec::with_capacity(item_keys.len());
    for key in &item_keys {
        let result = match client.update_item(key, None, &tags, None).await {
            Ok(_) => MoveResult {
                item_key: key.clone(),
                ok: true,
                error: None,
                collection_key: None,
                new_file_path: None,
            },
            Err(e) => MoveResult {
                item_key: key.clone(),
                ok: false,
                error: Some(e.to_string()),
                collection_key: None,
                new_file_path: None,
            },
        };
        results.push(result);
    }
    Ok(results)
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
            get_all_summaries,
            get_reading_states,
            set_reading_state,
            fetch_citation_graph,
            get_usage_summary,
            summarize_item,
            summarize_items,
            save_summary_note,
            chat_with_item,
            chat_with_items,
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
            export_plugin_xpi,
            write_text_file,
            add_tags
        ])
        .run(tauri::generate_context!())
        .expect("error while running Zotero Notebook");
}
