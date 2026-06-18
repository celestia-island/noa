use std::path::PathBuf;

use anyhow::Result;

use crate::coauthor::{
    provider::{AporiaProviderEntry, ProviderMap},
    session, build_report,
};

pub struct ResolveArgs {
    pub repo: PathBuf,
    pub chat_log_dir: Option<PathBuf>,
    pub aporia_config: Option<PathBuf>,
    pub lookback_secs: u64,
}

pub fn run(args: ResolveArgs) -> Result<()> {
    let provider_map = load_provider_map(&args.aporia_config);
    let chat_dir = args
        .chat_log_dir
        .clone()
        .or_else(session::default_chat_log_dir);
    let summary = match chat_dir {
        Some(dir) => session::summarize_chat_log_dir(&dir, args.lookback_secs),
        None => session::SessionSummary::default(),
    };
    let report = build_report(&summary.models, summary.yolo, &provider_map);
    if report.is_empty() {
        return Ok(());
    }
    let block = report.render_trailer_block();
    if !block.is_empty() {
        println!("{block}");
    }
    Ok(())
}

fn load_provider_map(aporia_config: &Option<PathBuf>) -> ProviderMap {
    let mut map = ProviderMap::builtin();
    if let Some(path) = aporia_config {
        if let Ok(content) = std::fs::read_to_string(path) {
            merge_aporia_content(&mut map, &content);
        }
    } else if let Some(auto) = locate_aporia_config() {
        if let Ok(content) = std::fs::read_to_string(&auto) {
            merge_aporia_content(&mut map, &content);
        }
    }
    map
}

fn locate_aporia_config() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("APORIA_CONFIG") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        let candidate = PathBuf::from(home)
            .join(".config")
            .join("entelecheia")
            .join("aporia.toml");
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn merge_aporia_content(map: &mut ProviderMap, content: &str) {
    #[derive(serde::Deserialize)]
    struct Config {
        #[serde(rename = "llm_providers", default)]
        llm_providers: Vec<RawProvider>,
    }
    #[derive(serde::Deserialize)]
    struct RawProvider {
        #[serde(default)]
        name: String,
        #[serde(default)]
        model: String,
        #[serde(default)]
        endpoint: String,
    }
    if let Ok(cfg) = toml::from_str::<Config>(content) {
        let entries: Vec<AporiaProviderEntry> = cfg
            .llm_providers
            .into_iter()
            .map(|p| AporiaProviderEntry {
                name: p.name,
                model: p.model,
                endpoint: p.endpoint,
            })
            .collect();
        map.merge_aporia(&entries);
    }
}
