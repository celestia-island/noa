pub mod provider;
pub mod session;

use serde::{Deserialize, Serialize};

use crate::coauthor::provider::resolve_provider;

pub const CELESTIA_DOMAIN: &str = "celestia.world";

pub const YOLO_AUTHORITY_DISPLAY: &str = "Entelecheia";
pub const YOLO_AUTHORITY_PROVIDER: &str = "demiurge";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoAuthor {
    pub display_name: String,
    pub provider_id: String,
}

impl CoAuthor {
    #[must_use]
    pub fn to_trailer(&self) -> String {
        format!(
            "Co-authored-by: {} <{}@{}>",
            self.display_name, self.provider_id, CELESTIA_DOMAIN
        )
    }

    #[must_use]
    pub fn yolo_authority() -> Self {
        Self {
            display_name: YOLO_AUTHORITY_DISPLAY.to_string(),
            provider_id: YOLO_AUTHORITY_PROVIDER.to_string(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelUsage {
    pub display_name: String,
    pub provider_id: String,
    pub model_id: String,
    pub upload: u64,
    pub download: u64,
    pub cache: Option<u64>,
}

impl ModelUsage {
    #[must_use]
    pub fn to_usage_line(&self) -> String {
        let cache_part = match self.cache {
            Some(c) if c > 0 => format!(", Cache {}", format_k(c)),
            _ => String::new(),
        };
        format!(
            "[{}] Upload {}, Download {}{}",
            self.display_name,
            format_k(self.upload),
            format_k(self.download),
            cache_part
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoAuthorReport {
    pub yolo: bool,
    pub authors: Vec<CoAuthor>,
    pub usage: Vec<ModelUsage>,
}

impl CoAuthorReport {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.authors.is_empty()
    }

    #[must_use]
    pub fn render_trailer_block(&self) -> String {
        if self.authors.is_empty() {
            return String::new();
        }
        let mut lines: Vec<String> = self.authors.iter().map(|a| a.to_trailer()).collect();
        if !self.usage.is_empty() {
            lines.push(String::new());
            lines.push("Token usage:".to_string());
            for u in &self.usage {
                lines.push(u.to_usage_line());
            }
        }
        lines.join("\n")
    }
}

#[must_use]
pub fn format_k(n: u64) -> String {
    if n == 0 {
        return "0".to_string();
    }
    let k = n as f64 / 1000.0;
    if k < 0.1 {
        format!("{:.2}", k)
    } else {
        format!("{:.1}", k)
    }
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
        + "k"
}

pub fn build_report(
    models: &[session::ModelRecord],
    yolo: bool,
    provider_map: &provider::ProviderMap,
) -> CoAuthorReport {
    let mut authors: Vec<CoAuthor> = Vec::new();
    if yolo {
        authors.push(CoAuthor::yolo_authority());
    }
    let mut usage: Vec<ModelUsage> = Vec::new();
    for m in models {
        let identity = resolve_provider(&m.model_id, provider_map);
        let display_name = identity
            .display_name
            .clone()
            .unwrap_or_else(|| m.model_id.clone());
        let provider_id = identity.provider_id.clone();
        let author = CoAuthor {
            display_name: display_name.clone(),
            provider_id: provider_id.clone(),
        };
        if !authors.iter().any(|a| {
            a.display_name == author.display_name && a.provider_id == author.provider_id
        }) {
            authors.push(author);
        }
        usage.push(ModelUsage {
            display_name,
            provider_id,
            model_id: m.model_id.clone(),
            upload: m.upload,
            download: m.download,
            cache: m.cache,
        });
    }
    CoAuthorReport {
        yolo,
        authors,
        usage,
    }
}
