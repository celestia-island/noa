use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct ProviderIdentity {
    pub display_name: Option<String>,
    pub provider_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderMap {
    pub by_model: HashMap<String, ProviderIdentity>,
}

impl ProviderMap {
    pub fn builtin() -> Self {
        let mut by_model: HashMap<String, ProviderIdentity> = HashMap::new();
        let mut add = |model_prefix: &str, display: &str, provider: &str| {
            by_model.insert(
                model_prefix.to_string(),
                ProviderIdentity {
                    display_name: Some(display.to_string()),
                    provider_id: provider.to_string(),
                },
            );
        };
        add("glm", "GLM", "zhipu.ai");
        add("deepseek", "Deepseek", "deepseek.com");
        add("claude", "Claude", "anthropic.com");
        add("gpt", "GPT", "openai.com");
        add("o1", "OpenAI", "openai.com");
        add("o3", "OpenAI", "openai.com");
        add("o4", "OpenAI", "openai.com");
        add("qwen", "Qwen", "dashscope.aliyuncs.com");
        add("gemini", "Gemini", "google.com");
        add("llama", "Llama", "meta.com");
        add("mistral", "Mistral", "mistral.ai");
        add("moonshot", "Kimi", "moonshot.cn");
        add("kimi", "Kimi", "moonshot.cn");
        add("yi", "Yi", "01.ai");
        add("baichuan", "Baichuan", "baichuan-ai.com");
        add("spark", "Spark", "xfyun.cn");
        add("ernie", "ERNIE", "baidu.com");
        add("hunyuan", "Hunyuan", "tencent.com");
        add("doubao", "Doubao", "volcengine.com");
        add("ark", "Doubao", "volcengine.com");
        Self { by_model }
    }

    pub fn merge_aporia(&mut self, providers: &[AporiaProviderEntry]) {
        for p in providers {
            if p.model.is_empty() {
                continue;
            }
            let provider_id = if !p.website_domain.is_empty() {
                p.website_domain.clone()
            } else {
                tracing::debug!(
                    model = %p.model,
                    provider = %p.name,
                    "aporia provider has no website_domain; skipping co-author attribution (website_domain is mandatory)"
                );
                continue;
            };
            let display = derive_display_name(&p.model, &provider_id);
            self.by_model.insert(
                p.model.clone(),
                ProviderIdentity {
                    display_name: Some(display),
                    provider_id,
                },
            );
        }
    }
}

#[derive(Debug, Clone)]
pub struct AporiaProviderEntry {
    pub name: String,
    pub model: String,
    pub endpoint: String,
    pub website_domain: String,
}

pub fn resolve_provider(model_id: &str, map: &ProviderMap) -> Option<ProviderIdentity> {
    let lower = model_id.to_ascii_lowercase();
    if let Some(id) = map.by_model.get(model_id).or_else(|| map.by_model.get(&lower)) {
        return Some(id.clone());
    }
    for (key, id) in &map.by_model {
        if lower.starts_with(key) {
            return Some(id.clone());
        }
    }
    None
}

pub fn endpoint_to_provider_id(endpoint: &str) -> Option<String> {
    let host = endpoint
        .split("://")
        .nth(1)
        .unwrap_or(endpoint)
        .split('/')
        .next()?
        .split(':')
        .next()?;
    let host = host.to_ascii_lowercase();
    let bare = host.trim_start_matches("api.").trim_start_matches("www.");
    let third_party = [
        "opencode.ai",
        "openrouter.ai",
        "jdcloud.com",
        "together.xyz",
        "together.ai",
        "fireworks.ai",
        "groq.com",
        "replicate.com",
        "anyscale.com",
        "lepton.ai",
        "siliconflow.cn",
        "volcengine.com",
        "dashscope.aliyuncs.com",
        "bigmodel.cn",
    ];
    if third_party.contains(&bare) {
        return Some(bare.to_string());
    }
    let first_party = [
        "deepseek.com",
        "anthropic.com",
        "openai.com",
        "google.com",
        "googleapis.com",
        "zhipu.ai",
        "mistral.ai",
        "meta.com",
        "moonshot.cn",
        "01.ai",
        "baichuan-ai.com",
        "xfyun.cn",
        "baidu.com",
        "tencent.com",
        "dashscope.aliyuncs.com",
    ];
    if first_party.contains(&bare) {
        return Some(normalize_first_party(bare));
    }
    if bare == "bigmodel.cn" {
        return Some("zhipu.ai".to_string());
    }
    None
}

fn normalize_first_party(host: &str) -> String {
    match host {
        "googleapis.com" => "google.com".to_string(),
        "dashscope.aliyuncs.com" => "dashscope.aliyuncs.com".to_string(),
        other => other.to_string(),
    }
}

pub fn derive_provider_id_from_model(model_id: &str) -> Option<String> {
    let lower = model_id.to_ascii_lowercase();
    let prefixes: &[(&str, &str)] = &[
        ("glm", "zhipu.ai"),
        ("deepseek", "deepseek.com"),
        ("claude", "anthropic.com"),
        ("gpt", "openai.com"),
        ("qwen", "dashscope.aliyuncs.com"),
        ("gemini", "google.com"),
        ("llama", "meta.com"),
        ("mistral", "mistral.ai"),
        ("mixtral", "mistral.ai"),
        ("moonshot", "moonshot.cn"),
        ("kimi", "moonshot.cn"),
        ("yi", "01.ai"),
        ("baichuan", "baichuan-ai.com"),
        ("spark", "xfyun.cn"),
        ("ernie", "baidu.com"),
        ("wenxin", "baidu.com"),
        ("hunyuan", "tencent.com"),
        ("doubao", "volcengine.com"),
    ];
    for (prefix, provider) in prefixes {
        if lower.starts_with(prefix) {
            return Some((*provider).to_string());
        }
    }
    None
}

pub fn derive_display_name(model_id: &str, provider_id: &str) -> String {
    let lower = model_id.to_ascii_lowercase();
    let brand = match provider_id {
        "zhipu.ai" => "GLM",
        "deepseek.com" => "Deepseek",
        "anthropic.com" => "Claude",
        "openai.com" => {
            if lower.starts_with("o1") || lower.starts_with("o3") || lower.starts_with("o4") {
                "OpenAI"
            } else {
                "GPT"
            }
        },
        "google.com" => "Gemini",
        "meta.com" => "Llama",
        "mistral.ai" => "Mistral",
        "moonshot.cn" => "Kimi",
        "01.ai" => "Yi",
        "baichuan-ai.com" => "Baichuan",
        "xfyun.cn" => "Spark",
        "baidu.com" => "ERNIE",
        "tencent.com" => "Hunyuan",
        "volcengine.com" => "Doubao",
        "dashscope.aliyuncs.com" => "Qwen",
        _ => "",
    };
    let rest_raw = if !brand.is_empty() {
        lower.strip_prefix(&lower.chars().take(brand.len()).collect::<String>())
    } else {
        None
    };
    let rest = rest_raw.unwrap_or(&lower);
    let rest = rest.trim_start_matches('-').trim_start_matches('_');
    let mut parts: Vec<String> = Vec::new();
    if !brand.is_empty() {
        parts.push(brand.to_string());
    }
    for chunk in rest.split(|c: char| c == '-' || c == '_' || c == '.') {
        if chunk.is_empty() {
            continue;
        }
        if let Ok(num) = chunk.parse::<f64>() {
            parts.push(format_trimmed_number(num));
        } else {
            parts.push(capitalize_chunk(chunk));
        }
    }
    if parts.is_empty() {
        capitalize_chunk(model_id)
    } else {
        parts.join(" ")
    }
}

fn format_trimmed_number(n: f64) -> String {
    let s = format!("{}", n);
    s
}

fn capitalize_chunk(chunk: &str) -> String {
    let mut out = String::with_capacity(chunk.len());
    for (i, c) in chunk.chars().enumerate() {
        if i == 0 {
            out.extend(c.to_uppercase());
        } else {
            out.extend(c.to_lowercase());
        }
    }
    if out.is_empty() {
        chunk.to_string()
    } else {
        out
    }
}
