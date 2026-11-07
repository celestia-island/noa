use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelRecord {
    pub model_id: String,
    pub upload: u64,
    pub download: u64,
    pub cache: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionSummary {
    pub models: Vec<ModelRecord>,
    pub yolo: bool,
}

pub fn default_chat_log_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ENTELECHEIA_CHAT_LOG_DIR") {
        return Some(PathBuf::from(p));
    }
    if let Some(home) = dirs_or_home() {
        return Some(home.join(".config").join("entelecheia").join("chat_logs"));
    }
    None
}

fn dirs_or_home() -> Option<PathBuf> {
    if let Ok(h) = std::env::var("HOME") {
        if !h.is_empty() {
            return Some(PathBuf::from(h));
        }
    }
    None
}

pub fn yolo_active_marker() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("/run/entelecheia/yolo_active"),
        PathBuf::from("/tmp/entelecheia-yolo-active"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

pub fn summarize_chat_log_dir(dir: &Path, lookback_secs: u64) -> SessionSummary {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e.flatten().collect::<Vec<_>>(),
        Err(_) => return SessionSummary::default(),
    };
    let mut logs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .into_iter()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !(name.starts_with("chat#") && name.ends_with(".log")) {
                return None;
            }
            e.metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| (t, e.path()))
        })
        .collect();
    logs.sort_by_key(|b| std::cmp::Reverse(b.0));

    let now = std::time::SystemTime::now();
    let cutoff: Option<std::time::Duration> = if lookback_secs > 0 {
        Some(std::time::Duration::from_secs(lookback_secs))
    } else {
        None
    };
    let now_age = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();

    let mut models_map: std::collections::HashMap<String, ModelRecord> =
        std::collections::HashMap::new();
    let mut yolo = false;

    let max_files = if lookback_secs == 0 { 4 } else { 6 };
    let mut found_model_data = false;
    for (_mtime, path) in logs.iter().take(max_files) {
        if let Some(window) = cutoff {
            if let Ok(file_time) = std::fs::metadata(path).and_then(|m| m.modified()) {
                if let Ok(file_age) = file_time.duration_since(std::time::UNIX_EPOCH) {
                    if now_age.saturating_sub(file_age) > window {
                        continue;
                    }
                }
            }
        }
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if content.contains("YOLO cruise control") || content.contains("YOLO auto") {
            yolo = true;
        }
        let entries = parse_chat_log_entries(&content);
        let log_total_tokens: u64 = entries
            .iter()
            .map(|e| e.upload.saturating_add(e.download))
            .sum();
        let has_models = entries.iter().any(|e| !e.model.is_empty()) && log_total_tokens > 0;
        for entry in &entries {
            if entry.model.is_empty() {
                continue;
            }
            let rec = models_map
                .entry(entry.model.clone())
                .or_insert_with(|| ModelRecord {
                    model_id: entry.model.clone(),
                    ..Default::default()
                });
            rec.upload = rec.upload.saturating_add(entry.upload);
            rec.download = rec.download.saturating_add(entry.download);
            if let Some(c) = entry.cache {
                rec.cache = Some(rec.cache.unwrap_or(0).saturating_add(c));
            }
        }
        if has_models {
            found_model_data = true;
            if lookback_secs == 0 {
                break;
            }
        }
    }
    if !found_model_data {
        models_map.clear();
    }

    let mut models: Vec<ModelRecord> = models_map.into_values().collect();
    models.sort_by(|a, b| a.model_id.cmp(&b.model_id));
    if models.is_empty() && yolo_active_marker().is_some() {
        yolo = true;
    }
    SessionSummary { models, yolo }
}

#[derive(Debug, Default)]
struct ParsedEntry {
    model: String,
    upload: u64,
    download: u64,
    cache: Option<u64>,
}

fn parse_chat_log_entries(content: &str) -> Vec<ParsedEntry> {
    let mut out = Vec::new();
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed != "+++" {
            continue;
        }
        let mut header = String::new();
        for hline in lines.by_ref() {
            if hline.trim() == "+++" {
                break;
            }
            header.push_str(hline);
            header.push('\n');
        }
        if header.contains("model =") {
            if let Some(entry) = parse_header(&header) {
                out.push(entry);
            }
        }
    }
    out
}

fn parse_header(header: &str) -> Option<ParsedEntry> {
    let map: std::collections::HashMap<String, String> = toml::from_str::<toml::Value>(header)
        .ok()
        .and_then(|v| v.as_table().cloned())?
        .into_iter()
        .filter_map(|(k, v)| {
            let s = match &v {
                toml::Value::String(s) => s.clone(),
                toml::Value::Integer(i) => i.to_string(),
                toml::Value::Boolean(b) => b.to_string(),
                toml::Value::Float(f) => f.to_string(),
                _ => return None,
            };
            Some((k, s))
        })
        .collect();
    let model = map.get("model").cloned()?;
    if model.is_empty() || model == "null" {
        return None;
    }
    let upload = map
        .get("input_tokens")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let download = map
        .get("output_tokens")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);
    let cache = map
        .get("cache_read_input_tokens")
        .or_else(|| map.get("cached_tokens"))
        .or_else(|| map.get("cache_tokens"))
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|c| *c > 0);
    Some(ParsedEntry {
        model,
        upload,
        download,
        cache,
    })
}
