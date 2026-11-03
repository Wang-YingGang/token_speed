use chrono::DateTime;
use serde::Serialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

const MAX_LINE: usize = 2 * 1024 * 1024;
const INITIAL_TAIL: u64 = 2 * 1024 * 1024;
const ENTRY_LIMIT: usize = 60;
const TEXT_LIMIT: usize = 6000;
const STALE_MS: i64 = 10 * 60 * 1000;
const RECENT_MS: i64 = 2 * 60 * 1000;
const RETAIN_MS: i64 = 60 * 60 * 1000;

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot {
    pub sessions: Vec<SessionView>,
    pub sources: Vec<SessionSource>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionSource {
    app_type: String,
    path: String,
    available: bool,
    error: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionView {
    id: String,
    app_type: String,
    project: String,
    cwd: String,
    status: String,
    started_at: Option<i64>,
    last_active_at: i64,
    preview: String,
    entries: Vec<OutputEntry>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OutputEntry {
    id: String,
    kind: String,
    title: String,
    text: String,
    timestamp: i64,
}

struct SessionState {
    view: SessionView,
    turn_id: String,
    calls: HashMap<String, String>,
}

fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("")
}

fn timestamp(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .map(|n| if n < 1_000_000_000_000 { n * 1000 } else { n })
        .or_else(|| {
            value
                .as_str()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|t| t.timestamp_millis())
        })
}

fn limit(text: &str, maximum: usize) -> String {
    let mut chars = text.chars();
    let result: String = chars.by_ref().take(maximum).collect();
    if chars.next().is_some() {
        format!("{result}\n…（内容已截断）")
    } else {
        result
    }
}

fn blocks_text(value: &Value) -> String {
    if let Some(text) = value.as_str() {
        return text.to_string();
    }
    value
        .as_array()
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| matches!(string(b, "type"), "text" | "output_text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

impl SessionState {
    fn new(app: &str, path: &Path) -> Self {
        Self {
            view: SessionView {
                id: path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                app_type: app.into(),
                project: "未知项目".into(),
                cwd: String::new(),
                status: "unknown".into(),
                started_at: None,
                last_active_at: 0,
                preview: String::new(),
                entries: vec![],
            },
            turn_id: String::new(),
            calls: HashMap::new(),
        }
    }

    fn cwd(&mut self, cwd: &str) {
        if cwd.is_empty() {
            return;
        }
        self.view.cwd = cwd.into();
        self.view.project = cwd
            .trim_end_matches(['/', '\\'])
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(cwd)
            .into();
    }

    fn entry(&mut self, id: &str, kind: &str, title: &str, text: &str, time: i64) {
        if text.trim().is_empty() {
            return;
        }
        let id = if id.is_empty() {
            format!("{kind}-{time}-{}", self.view.entries.len())
        } else {
            id.into()
        };
        let entry = OutputEntry {
            id: id.clone(),
            kind: kind.into(),
            title: limit(title, 180),
            text: limit(text, TEXT_LIMIT),
            timestamp: time,
        };
        if let Some(existing) = self.view.entries.iter_mut().find(|e| e.id == id) {
            *existing = entry;
        } else {
            self.view.entries.push(entry);
            if self.view.entries.len() > ENTRY_LIMIT {
                self.view.entries.remove(0);
            }
        }
        self.view.last_active_at = self.view.last_active_at.max(time);
        if self.view.status == "unknown" {
            self.view.status = "active".into();
        }
        if kind == "assistant" {
            self.view.preview = limit(text, 240);
        }
    }

    fn start(&mut self, turn: &str, time: i64, reliable: bool) {
        self.turn_id = turn.into();
        self.view.started_at = Some(time);
        self.view.last_active_at = time;
        self.view.status = if reliable { "running" } else { "active" }.into();
        self.view.preview.clear();
        self.calls.clear();
    }

    fn apply(&mut self, row: &Value) {
        if self.view.app_type == "claude" {
            self.claude(row);
        } else {
            self.codex(row);
        }
    }

    fn codex(&mut self, row: &Value) {
        let p = &row["payload"];
        let time = timestamp(&row["timestamp"]).unwrap_or(0);
        // Heartbeats refresh a known running turn without exposing reasoning data.
        if self.view.status == "running" {
            self.view.last_active_at = self.view.last_active_at.max(time);
        }
        match (string(row, "type"), string(p, "type")) {
            ("session_meta", _) => {
                let id = string(p, "id");
                if !id.is_empty() {
                    self.view.id = id.into();
                }
                self.cwd(string(p, "cwd"));
            }
            ("turn_context", _) => self.cwd(string(p, "cwd")),
            ("event_msg", "task_started" | "turn_started") => {
                self.start(
                    string(p, "turn_id"),
                    timestamp(&p["started_at"]).unwrap_or(time),
                    true,
                );
            }
            ("event_msg", "task_complete" | "task_completed" | "turn_aborted") => {
                let turn = string(p, "turn_id");
                if turn.is_empty() || self.turn_id.is_empty() || self.turn_id == turn {
                    self.view.status = if string(p, "type") == "turn_aborted" {
                        "unknown"
                    } else {
                        "completed"
                    }
                    .into();
                    self.view.last_active_at = self.view.last_active_at.max(time);
                }
            }
            ("response_item", "message") if string(p, "role") == "assistant" => {
                let phase = p
                    .get("phase")
                    .or_else(|| p.get("channel"))
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if matches!(phase, "commentary" | "final_answer" | "final" | "") {
                    self.entry(
                        string(p, "id"),
                        "assistant",
                        "回复",
                        &blocks_text(&p["content"]),
                        time,
                    );
                }
            }
            ("event_msg", "agent_message") => self.entry(
                string(p, "id"),
                "assistant",
                "回复",
                string(p, "message"),
                time,
            ),
            ("event_msg", "item_completed") => {
                let item = &p["item"];
                match string(item, "type") {
                    "AgentMessage"
                        if matches!(
                            string(item, "phase"),
                            "commentary" | "final_answer" | "final" | ""
                        ) =>
                    {
                        self.entry(
                            string(item, "id"),
                            "assistant",
                            "回复",
                            &blocks_text(&item["content"]),
                            time,
                        );
                    }
                    "CommandExecution" => {
                        let command = item["command"]
                            .as_array()
                            .map(|parts| {
                                parts
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            })
                            .unwrap_or_else(|| string(item, "command").into());
                        let output = string(item, "aggregated_output");
                        self.entry(
                            string(item, "id"),
                            "tool",
                            if command.is_empty() {
                                "命令结果"
                            } else {
                                &command
                            },
                            output,
                            time,
                        );
                    }
                    _ => {}
                }
            }
            ("response_item", "function_call" | "custom_tool_call") => {
                let name = string(p, "name");
                if self.calls.len() >= 128 {
                    self.calls.clear();
                }
                self.calls.insert(string(p, "call_id").into(), name.into());
                self.entry(
                    &format!("call-{}", string(p, "call_id")),
                    "tool",
                    "调用工具",
                    name,
                    time,
                );
            }
            ("response_item", "function_call_output" | "custom_tool_call_output") => {
                let id = string(p, "call_id");
                let title = self
                    .calls
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| "工具结果".into());
                let output = p
                    .get("output")
                    .map(|v| {
                        if v.is_string() {
                            string(p, "output").into()
                        } else {
                            blocks_text(v)
                        }
                    })
                    .unwrap_or_default();
                self.entry(&format!("result-{id}"), "tool", &title, &output, time);
            }
            _ => {}
        }
    }

    fn claude(&mut self, row: &Value) {
        // Subagent files reuse the parent session ID; don't let their output
        // replace the main conversation when snapshots are merged by ID.
        if row["isSidechain"].as_bool() == Some(true) {
            return;
        }
        let time = timestamp(&row["timestamp"]).unwrap_or(0);
        let id = string(row, "sessionId");
        if !id.is_empty() {
            self.view.id = id.into();
        }
        self.cwd(string(row, "cwd"));
        let content = &row["message"]["content"];
        let blocks = content.as_array();
        match string(row, "type") {
            "user" => {
                let result_only =
                    blocks.is_some_and(|b| b.iter().all(|v| string(v, "type") == "tool_result"));
                if !result_only {
                    self.start(string(row, "uuid"), time, false);
                }
                if let Some(blocks) = blocks {
                    for b in blocks.iter().filter(|v| string(v, "type") == "tool_result") {
                        let call = string(b, "tool_use_id");
                        let title = self
                            .calls
                            .get(call)
                            .cloned()
                            .unwrap_or_else(|| "工具结果".into());
                        self.entry(
                            &format!("result-{call}"),
                            "tool",
                            &title,
                            &blocks_text(&b["content"]),
                            time,
                        );
                    }
                }
            }
            "assistant" => {
                let uuid = string(row, "uuid");
                if let Some(blocks) = blocks {
                    for (index, b) in blocks.iter().enumerate() {
                        match string(b, "type") {
                            "text" => self.entry(
                                &format!("{uuid}-{index}"),
                                "assistant",
                                "回复",
                                string(b, "text"),
                                time,
                            ),
                            "tool_use" => {
                                if self.calls.len() >= 128 {
                                    self.calls.clear();
                                }
                                self.calls
                                    .insert(string(b, "id").into(), string(b, "name").into());
                                self.entry(
                                    string(b, "id"),
                                    "tool",
                                    "调用工具",
                                    string(b, "name"),
                                    time,
                                );
                            }
                            _ => {}
                        }
                    }
                }
                self.view.last_active_at = self.view.last_active_at.max(time);
                self.view.status = if matches!(
                    string(&row["message"], "stop_reason"),
                    "end_turn" | "stop_sequence"
                ) {
                    "completed"
                } else {
                    "active"
                }
                .into();
            }
            _ => {}
        }
    }

    fn snapshot(&self, now: i64) -> SessionView {
        let mut view = self.view.clone();
        let age = now.saturating_sub(view.last_active_at);
        if (view.status == "running" && age > STALE_MS)
            || (view.status == "active" && age > RECENT_MS)
        {
            view.status = "unknown".into();
        }
        view
    }
}

struct SessionFile {
    state: SessionState,
    offset: u64,
    pending: Vec<u8>,
    prefix: Vec<u8>,
    dropping: bool,
    initialized: bool,
    error: Option<String>,
}

impl SessionFile {
    fn new(app: &str, path: &Path) -> Self {
        Self {
            state: SessionState::new(app, path),
            offset: 0,
            pending: vec![],
            prefix: vec![],
            dropping: false,
            initialized: false,
            error: None,
        }
    }

    fn consume(&mut self, bytes: &[u8]) {
        for segment in bytes.split_inclusive(|b| *b == b'\n') {
            let complete = segment.last() == Some(&b'\n');
            if self.pending.len() + segment.len() > MAX_LINE {
                self.pending.clear();
                self.dropping = true;
            }
            if !self.dropping {
                self.pending.extend_from_slice(segment);
            }
            if complete {
                if !self.dropping {
                    if let Ok(row) = serde_json::from_slice::<Value>(&self.pending) {
                        self.state.apply(&row);
                    }
                }
                self.pending.clear();
                self.dropping = false;
            }
        }
    }

    fn read(&mut self, path: &Path) -> std::io::Result<()> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        let mut prefix = vec![0; self.prefix.len().max(256).min(length as usize)];
        file.read_exact(&mut prefix)?;
        if self.initialized && (length < self.offset || !prefix.starts_with(&self.prefix)) {
            *self = Self::new(&self.state.view.app_type, path);
        }
        self.prefix = prefix;
        if !self.initialized {
            if length > INITIAL_TAIL {
                file.seek(SeekFrom::Start(0))?;
                let mut header = Vec::new();
                BufReader::new((&mut file).take(MAX_LINE as u64)).read_until(b'\n', &mut header)?;
                self.consume(&header);
                self.pending.clear();
                self.offset = length - INITIAL_TAIL;
                // Discard the first tail fragment, which may start mid-line.
                self.dropping = true;
            }
            self.initialized = true;
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        (&mut file).take(INITIAL_TAIL).read_to_end(&mut bytes)?;
        self.offset += bytes.len() as u64;
        self.consume(&bytes);
        Ok(())
    }
}

pub struct SessionMonitor {
    roots: Vec<(String, PathBuf)>,
    files: HashMap<PathBuf, SessionFile>,
    discovery_errors: HashMap<String, String>,
}

fn discover(path: &Path, output: &mut HashSet<PathBuf>, depth: usize) -> std::io::Result<()> {
    if depth > 10 {
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        if ty.is_dir() {
            discover(&entry.path(), output, depth + 1)?;
        } else if ty.is_file() && entry.path().extension().is_some_and(|e| e == "jsonl") {
            output.insert(entry.path());
        }
    }
    Ok(())
}

impl SessionMonitor {
    pub fn from_environment() -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self::new(vec![
            (
                "codex".into(),
                std::env::var_os("TOKEN_SPEED_CODEX_SESSIONS")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".codex/sessions")),
            ),
            (
                "claude".into(),
                std::env::var_os("TOKEN_SPEED_CLAUDE_PROJECTS")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| home.join(".claude/projects")),
            ),
        ])
    }

    fn new(roots: Vec<(String, PathBuf)>) -> Self {
        Self {
            roots,
            files: HashMap::new(),
            discovery_errors: HashMap::new(),
        }
    }

    fn discover(&mut self) {
        for (app, root) in &self.roots {
            let mut paths = HashSet::new();
            self.discovery_errors.remove(app);
            if root.exists() {
                if let Err(e) = discover(root, &mut paths, 0) {
                    self.discovery_errors.insert(app.clone(), e.to_string());
                }
            }
            // Keep tracked files during transient directory errors so a single
            // unreadable directory cannot erase the last successful snapshot.
            if !self.discovery_errors.contains_key(app) {
                self.files
                    .retain(|path, f| f.state.view.app_type != *app || paths.contains(path));
            }
            for path in paths {
                self.files
                    .entry(path.clone())
                    .or_insert_with(|| SessionFile::new(app, &path));
            }
        }
    }

    fn poll(&mut self, now: i64) -> SessionSnapshot {
        for (path, file) in &mut self.files {
            file.error = file.read(path).err().map(|e| e.to_string());
        }
        let mut unique: HashMap<(String, String), SessionView> = HashMap::new();
        for file in self.files.values() {
            let mut view = file.state.snapshot(now);
            if file.error.is_some() {
                view.status = "unknown".into();
            }
            if view.last_active_at <= 0 || now.saturating_sub(view.last_active_at) > RETAIN_MS {
                continue;
            }
            let key = (view.app_type.clone(), view.id.clone());
            if unique
                .get(&key)
                .map_or(true, |old| view.last_active_at > old.last_active_at)
            {
                unique.insert(key, view);
            }
        }
        let mut sessions: Vec<_> = unique.into_values().collect();
        sessions.sort_by(|a, b| {
            let rank = |s: &str| match s {
                "running" => 0,
                "active" => 1,
                "completed" => 2,
                _ => 3,
            };
            rank(&a.status)
                .cmp(&rank(&b.status))
                .then(b.last_active_at.cmp(&a.last_active_at))
                .then(a.id.cmp(&b.id))
        });
        sessions.truncate(30);
        let sources = self
            .roots
            .iter()
            .map(|(app, path)| SessionSource {
                app_type: app.clone(),
                path: path.display().to_string(),
                available: path.is_dir(),
                error: self.discovery_errors.get(app).cloned().or_else(|| {
                    self.files
                        .values()
                        .find(|f| f.state.view.app_type == *app && f.error.is_some())
                        .and_then(|f| f.error.clone())
                }),
            })
            .collect();
        SessionSnapshot { sessions, sources }
    }
}

#[tauri::command]
pub fn get_sessions(
    state: tauri::State<'_, RwLock<SessionSnapshot>>,
) -> Result<SessionSnapshot, String> {
    state
        .read()
        .map(|v| v.clone())
        .map_err(|_| "无法读取会话".into())
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        let mut monitor = SessionMonitor::from_environment();
        let mut last_discovery = Instant::now();
        monitor.discover();
        loop {
            if last_discovery.elapsed() >= Duration::from_secs(5) {
                monitor.discover();
                last_discovery = Instant::now();
            }
            let snapshot = monitor.poll(crate::now_ms());
            if let Ok(mut state) = app.state::<RwLock<SessionSnapshot>>().write() {
                if *state != snapshot {
                    *state = snapshot.clone();
                    let _ = app.emit("sessions-changed", &snapshot);
                }
            }
            std::thread::sleep(Duration::from_millis(750));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const NOW: i64 = 1_793_600_000_000;
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("target/session-tests")
                .join(format!(
                    "{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn event(kind: &str, extra: Value) -> Value {
        let mut payload = extra;
        payload["type"] = kind.into();
        json!({"type":"event_msg","timestamp":NOW,"payload":payload})
    }
    fn message(id: &str, text: &str) -> Value {
        json!({"type":"response_item","timestamp":NOW,"payload":{"type":"message","role":"assistant","phase":"commentary","id":id,"content":[{"type":"output_text","text":text}]}})
    }
    fn write_rows(path: &Path, rows: &[Value]) {
        let text = rows.iter().map(|r| format!("{r}\n")).collect::<String>();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn codex_messages_are_deduplicated_and_do_not_finish_the_turn() {
        let mut state = SessionState::new("codex", Path::new("sample.jsonl"));
        state.apply(&event("task_started", json!({"turn_id":"t"})));
        state.apply(&event("item_completed", json!({"item":{"type":"AgentMessage","id":"m","phase":"commentary","content":[{"type":"text","text":"正在检查"}]}})));
        state.apply(&message("m", "正在检查"));
        state.apply(&json!({"type":"response_item","timestamp":NOW,"payload":{"type":"reasoning","summary":[{"text":"private"}]}}));
        state.apply(&json!({"type":"response_item","timestamp":NOW,"payload":{"type":"message","role":"assistant","phase":"analysis","content":[{"type":"output_text","text":"private"}]}}));
        assert_eq!(state.view.entries.len(), 1);
        assert_eq!(state.view.preview, "正在检查");
        assert_eq!(state.snapshot(NOW).status, "running");
        state.apply(&event("task_complete", json!({"turn_id":"older"})));
        assert_eq!(state.view.status, "running");
        state.apply(&event("task_complete", json!({"turn_id":"t"})));
        assert_eq!(state.view.status, "completed");
    }

    #[test]
    fn stale_turn_is_unknown_and_next_turn_clears_preview() {
        let mut state = SessionState::new("codex", Path::new("sample.jsonl"));
        state.apply(&event("task_started", json!({"turn_id":"t"})));
        state.apply(&message("m", "上一轮"));
        assert_eq!(state.snapshot(NOW + STALE_MS + 1).status, "unknown");
        state.apply(&event("task_started", json!({"turn_id":"next"})));
        assert!(state.view.preview.is_empty());
        assert_eq!(state.view.entries.len(), 1);
    }

    #[test]
    fn partial_utf8_and_invalid_lines_do_not_lose_following_output() {
        let mut file = SessionFile::new("codex", Path::new("sample.jsonl"));
        let text = format!("{}\n", message("m", "你好，输出"));
        let split = text.find('你').unwrap() + 1;
        file.consume(&text.as_bytes()[..split]);
        assert!(file.state.view.entries.is_empty());
        file.consume(&text.as_bytes()[split..]);
        file.consume(b"malformed\n");
        file.consume(format!("{}\n", message("n", "继续")).as_bytes());
        assert_eq!(file.state.view.entries.len(), 2);
        assert_eq!(file.state.view.preview, "继续");
    }

    #[test]
    fn oversize_lines_are_discarded_and_output_is_bounded() {
        let mut file = SessionFile::new("codex", Path::new("sample.jsonl"));
        file.consume(&vec![b'x'; MAX_LINE + 1]);
        file.consume(b"\n");
        for i in 0..ENTRY_LIMIT + 5 {
            file.consume(
                format!(
                    "{}\n",
                    message(&i.to_string(), &"字".repeat(TEXT_LIMIT + 1))
                )
                .as_bytes(),
            );
        }
        assert_eq!(file.state.view.entries.len(), ENTRY_LIMIT);
        assert!(file.state.view.entries[0].text.chars().count() < TEXT_LIMIT + 30);
        assert!(file.pending.is_empty());
    }

    #[test]
    fn file_append_truncation_and_equal_length_replacement_are_detected() {
        let fixture = Fixture::new();
        let path = fixture.0.join("feed.jsonl");
        write_rows(&path, &[message("one", "first")]);
        let mut file = SessionFile::new("codex", &path);
        file.read(&path).unwrap();
        let mut handle = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(handle, "{}", message("two", "second")).unwrap();
        drop(handle);
        file.read(&path).unwrap();
        assert_eq!(file.state.view.entries.len(), 2);
        write_rows(&path, &[message("new", "other")]);
        file.read(&path).unwrap();
        assert_eq!(file.state.view.entries.len(), 1);
        assert_eq!(file.state.view.preview, "other");
        let length = fs::metadata(&path).unwrap().len();
        write_rows(&path, &[message("NEW", "OTHER")]);
        assert_eq!(fs::metadata(&path).unwrap().len(), length);
        file.read(&path).unwrap();
        assert_eq!(file.state.view.preview, "OTHER");
    }

    #[test]
    fn large_file_retains_metadata_and_reads_tail() {
        let fixture = Fixture::new();
        let path = fixture.0.join("large.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            "{}",
            json!({"type":"session_meta","payload":{"id":"session","cwd":"E:/code/project"}})
        )
        .unwrap();
        file.write_all(&vec![b'x'; INITIAL_TAIL as usize + 100])
            .unwrap();
        writeln!(file, "\n{}", message("tail", "尾部输出")).unwrap();
        drop(file);
        let mut reader = SessionFile::new("codex", &path);
        reader.read(&path).unwrap();
        assert_eq!(reader.state.view.id, "session");
        assert_eq!(reader.state.view.project, "project");
        assert_eq!(reader.state.view.preview, "尾部输出");
    }

    #[test]
    fn claude_tool_result_is_not_a_new_user_turn() {
        let mut state = SessionState::new("claude", Path::new("sample.jsonl"));
        state.apply(&json!({"type":"user","uuid":"user","sessionId":"s","cwd":"E:\\code\\project","timestamp":NOW,"message":{"content":"开始"}}));
        state.apply(&json!({"type":"assistant","uuid":"a","timestamp":NOW,"message":{"stop_reason":"tool_use","content":[{"type":"text","text":"正在测试"},{"type":"tool_use","id":"tool","name":"Bash"},{"type":"thinking","thinking":"private"}]}}));
        state.apply(&json!({"type":"user","uuid":"result","timestamp":NOW+1000,"message":{"content":[{"type":"tool_result","tool_use_id":"tool","content":[{"type":"text","text":"tests passed"}]}]}}));
        assert_eq!(state.turn_id, "user");
        assert_eq!(state.view.started_at, Some(NOW));
        assert_eq!(state.view.preview, "正在测试");
        assert_eq!(state.view.entries.len(), 3);
        assert_eq!(state.view.entries[2].title, "Bash");
        assert_eq!(state.snapshot(NOW + RECENT_MS + 2000).status, "unknown");
        state.apply(&json!({"type":"assistant","uuid":"done","timestamp":NOW+2000,"message":{"stop_reason":"end_turn","content":[{"type":"text","text":"完成"}]}}));
        assert_eq!(state.view.status, "completed");
        state.apply(&json!({"type":"assistant","isSidechain":true,"uuid":"child","timestamp":NOW+3000,"message":{"content":[{"type":"text","text":"子任务输出"}]}}));
        assert_eq!(state.view.preview, "完成");
        assert_eq!(state.view.status, "completed");
    }

    #[test]
    fn desktop_command_arrays_keep_the_command_title() {
        let mut state = SessionState::new("codex", Path::new("sample.jsonl"));
        state.apply(&event("item_completed", json!({"item":{"type":"CommandExecution","id":"command","command":["cargo","test"],"aggregated_output":"passed"}})));
        assert_eq!(state.view.entries[0].title, "cargo test");
        assert_eq!(state.view.entries[0].text, "passed");
    }

    #[test]
    fn discovery_finds_new_files_removes_deleted_and_ignores_old_sessions() {
        let fixture = Fixture::new();
        let path = fixture.0.join("feed.jsonl");
        let mut monitor = SessionMonitor::new(vec![
            ("codex".into(), fixture.0.clone()),
            ("claude".into(), fixture.0.join("missing")),
        ]);
        monitor.discover();
        assert!(monitor.poll(NOW).sessions.is_empty());
        write_rows(
            &path,
            &[
                event("task_started", json!({"turn_id":"t"})),
                message("m", "test"),
            ],
        );
        monitor.discover();
        let snapshot = monitor.poll(NOW);
        assert_eq!(snapshot.sessions.len(), 1);
        assert!(snapshot.sources[0].available);
        assert!(!snapshot.sources[1].available);
        assert!(monitor.poll(NOW + RETAIN_MS + 1).sessions.is_empty());
        fs::remove_file(&path).unwrap();
        monitor.discover();
        assert!(monitor.poll(NOW).sessions.is_empty());
    }

    #[test]
    fn parses_iso_timestamp_and_project_paths() {
        assert_eq!(
            timestamp(&json!("2026-11-02T14:45:36.473+08:00")),
            Some(1_793_601_936_473)
        );
        assert_eq!(timestamp(&json!(1_793_601_936)), Some(1_793_601_936_000));
        assert_eq!(timestamp(&json!("invalid")), None);
    }
}
