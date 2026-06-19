pub mod provider;
pub mod session;

use serde::{Deserialize, Serialize};

use crate::coauthor::provider::resolve_provider;

pub const CELESTIA_DOMAIN: &str = "celestia.world";

pub const YOLO_AUTHORITY_DISPLAY: &str = "Entelecheia";
pub const YOLO_AUTHORITY_PROVIDER: &str = "demiurge";

pub const UPLOAD_GLYPH: &str = "↑";
pub const DOWNLOAD_GLYPH: &str = "↓";
pub const CACHE_GLYPH: &str = "●";

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
    pub fn usage_inline(&self) -> String {
        let mut s = format!(
            "({UPLOAD_GLYPH} {} {DOWNLOAD_GLYPH} {}",
            format_k(self.upload),
            format_k(self.download)
        );
        if let Some(c) = self.cache {
            if c > 0 {
                s.push_str(&format!(" {CACHE_GLYPH}{}", format_k(c)));
            }
        }
        s.push(')');
        s
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
        let mut lines: Vec<String> = Vec::new();
        for a in &self.authors {
            let usage = self
                .usage
                .iter()
                .find(|u| u.display_name == a.display_name && u.provider_id == a.provider_id);
            let name = match usage {
                Some(u) => format!("{} {}", a.display_name, u.usage_inline()),
                None => a.display_name.clone(),
            };
            lines.push(format!(
                "Co-authored-by: {} <{}@{}>",
                name, a.provider_id, CELESTIA_DOMAIN
            ));
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
        let Some(identity) = resolve_provider(&m.model_id, provider_map) else {
            continue;
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_k_values() {
        assert_eq!(format_k(0), "0");
        assert_eq!(format_k(500), "0.5k");
        assert_eq!(format_k(1503), "1.5k");
        assert_eq!(format_k(36429), "36.4k");
        assert_eq!(format_k(1000), "1k");
        assert_eq!(format_k(100000), "100k");
    }

    #[test]
    fn test_coauthor_trailer_format() {
        let a = CoAuthor {
            display_name: "Claude Opus 4.8".to_string(),
            provider_id: "anthropic.com".to_string(),
        };
        assert_eq!(
            a.to_trailer(),
            "Co-authored-by: Claude Opus 4.8 <anthropic.com@celestia.world>"
        );
    }

    #[test]
    fn test_yolo_authority() {
        let a = CoAuthor::yolo_authority();
        assert_eq!(a.display_name, "Entelecheia");
        assert_eq!(a.provider_id, "demiurge");
        assert_eq!(
            a.to_trailer(),
            "Co-authored-by: Entelecheia <demiurge@celestia.world>"
        );
    }

    #[test]
    fn test_usage_inline_without_cache() {
        let u = ModelUsage {
            display_name: "GLM 5".to_string(),
            provider_id: "zhipuai.cn".to_string(),
            model_id: "glm-5".to_string(),
            upload: 36429,
            download: 1503,
            cache: None,
        };
        assert_eq!(u.usage_inline(), "(↑ 36.4k ↓ 1.5k)");
    }

    #[test]
    fn test_usage_inline_with_cache() {
        let u = ModelUsage {
            display_name: "Claude Opus 4.8".to_string(),
            provider_id: "anthropic.com".to_string(),
            model_id: "claude-opus-4-8".to_string(),
            upload: 12500,
            download: 8300,
            cache: Some(45200),
        };
        assert_eq!(u.usage_inline(), "(↑ 12.5k ↓ 8.3k ●45.2k)");
    }

    #[test]
    fn test_render_block_with_yolo() {
        let report = CoAuthorReport {
            yolo: true,
            authors: vec![
                CoAuthor::yolo_authority(),
                CoAuthor {
                    display_name: "GLM 5".to_string(),
                    provider_id: "zhipuai.cn".to_string(),
                },
            ],
            usage: vec![ModelUsage {
                display_name: "GLM 5".to_string(),
                provider_id: "zhipuai.cn".to_string(),
                model_id: "glm-5".to_string(),
                upload: 36429,
                download: 1503,
                cache: None,
            }],
        };
        let block = report.render_trailer_block();
        assert!(block.contains("Co-authored-by: Entelecheia <demiurge@celestia.world>"));
        assert!(block.contains("Co-authored-by: GLM 5 (↑ 36.4k ↓ 1.5k) <zhipuai.cn@celestia.world>"));
        assert!(!block.contains("Token usage:"));
    }

    #[test]
    fn test_render_empty_report() {
        let report = CoAuthorReport::default();
        assert!(report.is_empty());
        assert_eq!(report.render_trailer_block(), "");
    }
}

