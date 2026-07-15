use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};

const COMMIT_MSG_HOOK: &str = include_str!("../../assets/commit-msg.sh");

/// Sentinel string present in every noa-managed hook script.
/// Used to detect whether an existing hook file is safe to overwrite.
const NOA_MANAGED_SENTINEL: &str = "noa-managed";

pub struct InstallArgs {
    pub repo: PathBuf,
    pub force: bool,
    pub noa_bin: Option<String>,
}

<<<<<<< HEAD
=======
pub struct ValidateMsgArgs {
    pub message: String,
    pub preset: Option<String>,
    pub repo: PathBuf,
}

/// Validate a commit / PR-title message against a named preset.
///
/// Rules are loaded from `.noa/message-rules.toml` in the given repository
/// (or the built-in `celestia` preset when no config file exists). The preset
/// name defaults to `celestia`. Returns `Ok(())` when the message passes all
/// checks, or an error describing the first violation.
pub fn validate_msg(args: ValidateMsgArgs) -> Result<()> {
    let preset_name = args.preset.unwrap_or_else(|| "celestia".to_string());
    let rules = load_rules(&args.repo, &preset_name)?;
    let msg = args.message.trim();

    if msg.is_empty() {
        bail!("message is empty");
    }

    // 1. Gitmoji prefix check.
    if let Some(ref pattern) = rules.required_prefix_pattern {
        let re = regex::Regex::new(pattern)
            .with_context(|| format!("invalid prefix regex: {pattern}"))?;
        if !re.is_match_at(msg, 0) {
            bail!(
                "message must start with a gitmoji (pattern: {pattern})\n\
                 see https://gitmoji.dev for the full list"
            );
        }
    }

    // 2. After stripping emoji, first letter must be uppercase.
    let after_emoji = strip_leading_emoji(msg);
    if rules.require_capitalized && !after_emoji.is_empty() {
        let first_char = after_emoji.chars().next().unwrap();
        if first_char != first_char.to_uppercase().next().unwrap() {
            bail!("message must start with a capital letter after the gitmoji");
        }
    }

    // 3. Must end with a period (before any PR suffix).
    if rules.require_period {
        let base = strip_pr_suffix(after_emoji);
        if !base.trim_end().ends_with('.') {
            bail!("message must end with a period (.)");
        }
    }

    // 4. PR suffix format check.
    if let Some(ref _fmt) = rules.pr_suffix_format {
        let body = after_emoji.trim();
        if let Some(suffix_start) = body.rfind(" (#") {
            let suffix = &body[suffix_start..];
            if !suffix.ends_with(')') {
                bail!("PR reference must end with )");
            }
            let num_str = &suffix[3..suffix.len() - 1];
            if num_str.parse::<u64>().is_err() {
                bail!("PR reference must be a number: {suffix}");
            }
        }
    }

    Ok(())
}

/// Strip a leading emoji / multi-codepoint emoji sequence from `s`.
fn strip_leading_emoji(s: &str) -> &str {
    let mut indices = s.char_indices();
    while let Some((i, c)) = indices.next() {
        if c.is_ascii_whitespace() {
            continue;
        }
        if c as u32 >= 0x2000 || c.is_alphabetic() || c.is_ascii_punctuation() {
            return &s[i..];
        }
        if c.is_ascii() {
            return &s[i..];
        }
    }
    s
}

/// Remove a trailing ` (#999)` suffix from `s` and return the text before it.
fn strip_pr_suffix(s: &str) -> &str {
    let s = s.trim();
    if let Some(pos) = s.rfind(" (#") {
        if s[pos..].ends_with(')') {
            return s[..pos].trim_end();
        }
    }
    s
}

/// Load message validation rules for `preset_name` from `.noa/message-rules.toml`
/// in the given repository, falling back to the built-in `celestia` preset.
fn load_rules(repo: &Path, preset_name: &str) -> Result<MessageRules> {
    let config_path = repo.join(".noa").join("message-rules.toml");
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)
            .with_context(|| format!("reading {}", config_path.display()))?;
        let config: RulesConfig = toml::from_str(&content)
            .with_context(|| format!("parsing {}", config_path.display()))?;
        if let Some(rules) = config.preset.get(preset_name) {
            return Ok(rules.clone());
        }
    }
    // Fall back to built-in celestia preset.
    if preset_name == "celestia" {
        Ok(MessageRules::celestia())
    } else {
        bail!(
            "preset '{preset_name}' not found in {}",
            config_path.display()
        );
    }
}

#[derive(Debug, Clone, serde::Deserialize)]
struct RulesConfig {
    preset: std::collections::HashMap<String, MessageRules>,
}

#[derive(Debug, Clone, serde::Deserialize)]
struct MessageRules {
    #[serde(default)]
    required_prefix_pattern: Option<String>,
    #[serde(default)]
    require_capitalized: bool,
    #[serde(default)]
    require_period: bool,
    #[serde(default)]
    pr_suffix_format: Option<String>,
}

impl MessageRules {
    fn celestia() -> Self {
        Self {
            required_prefix_pattern: Some(r"^[\u{2600}-\u{27BF}\u{2B50}\u{1F300}-\u{1F9FF}\u{1FA00}-\u{1FA6F}\u{1FA70}-\u{1FAFF}\u{2702}-\u{27B0}\u{1F600}-\u{1F64F}]".to_string()),
            require_capitalized: true,
            require_period: true,
            pr_suffix_format: Some(" (#%d)".to_string()),
        }
    }
}

>>>>>>> origin/dev
/// Install noa-managed git hooks into the target repository's `.git/hooks/`
/// directory. Currently this is just `commit-msg` (AI co-author trailers).
///
/// Pre-commit build/secret validation used to live here but has moved to
/// entelecheia's review agent — it is not noa's responsibility. The
/// `noa hook pre-commit` subcommand is retained as a no-op so hooks installed
/// by older noa versions don't break (they just do nothing).
pub fn run(args: InstallArgs) -> Result<()> {
    let repo = if args.repo.is_absolute() {
        args.repo.clone()
    } else {
        std::env::current_dir()?.join(&args.repo)
    };
    let dot_git = repo.join(".git");
    let hooks_dir = if dot_git.is_dir() {
        dot_git.join("hooks")
    } else {
        bail!(
            "not a git repository (no .git directory at {})",
            dot_git.display()
        );
    };
    std::fs::create_dir_all(&hooks_dir)
        .with_context(|| format!("creating hooks dir {}", hooks_dir.display()))?;

    let noa_bin = args
        .noa_bin
        .clone()
        .or_else(detect_noa_bin)
        .unwrap_or_else(|| "noa".to_string());
    let resolved = if noa_bin.contains(' ') || std::path::Path::new(&noa_bin).exists() {
        noa_bin
    } else {
        which_noa(&noa_bin).unwrap_or(noa_bin)
    };

    install_hook(
        &hooks_dir,
        "commit-msg",
        COMMIT_MSG_HOOK,
        &resolved,
        args.force,
    )?;

    println!("Installed noa hooks into {}", hooks_dir.display());
    println!("Resolver: {resolved} co-author resolve");
    Ok(())
}

/// Write a single hook script into `hooks_dir/<name>`, substituting `@NOA_BIN@`
/// with the resolved noa binary path. Refuses to overwrite a non-noa-managed
/// hook unless `force` is set. Marks the file executable on Unix.
fn install_hook(
    hooks_dir: &Path,
    name: &str,
    template: &str,
    noa_bin: &str,
    force: bool,
) -> Result<()> {
    let hook_path = hooks_dir.join(name);
    if hook_path.exists() && !force {
        let existing = std::fs::read_to_string(&hook_path).unwrap_or_default();
        if !existing.contains(NOA_MANAGED_SENTINEL) {
            bail!(
                "{name} hook already exists at {} and is not managed by noa. \
                 Re-run with --force to overwrite.",
                hook_path.display()
            );
        }
    }

    let content = template.replace("@NOA_BIN@", noa_bin);
    std::fs::write(&hook_path, content)
        .with_context(|| format!("writing hook {}", hook_path.display()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&hook_path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&hook_path, perms)?;
    }

    println!("Installed {name} hook at {}", hook_path.display());
    Ok(())
}

/// Entry point for `noa hook pre-commit`.
///
/// Pre-commit build/secret validation has moved to entelecheia's review agent
/// and is no longer noa's responsibility. This subcommand is retained as a
/// **no-op** so `.git/hooks/pre-commit` scripts installed by older noa versions
/// keep working (they call into here, do nothing, and exit 0) instead of
/// breaking commits with an unknown-subcommand error. It is no longer
/// installed by `noa hook install`.
pub fn run_pre_commit() -> Result<()> {
    eprintln!(
        "[noa pre-commit] pre-commit checks have moved to the entelecheia review \
         agent; this hook is now a no-op and can be removed from .git/hooks."
    );
    Ok(())
}

<<<<<<< HEAD
=======
pub struct InstallActionArgs {
    pub repo: PathBuf,
    pub force: bool,
}

const PR_TITLE_CHECK_YML: &str = include_str!("../../assets/pr-title-check.yml");
const SQUASH_MSG_CLEAN_YML: &str = include_str!("../../assets/squash-msg-clean.yml");

/// Install noa's GitHub Action workflows into `.github/workflows/`.
///
/// Writes two workflow files:
/// - `pr-title-check.yml` — validates PR titles on open/edit/sync.
/// - `squash-msg-clean.yml` — amends squash-merge bodies to 1 line on push to master.
///
/// Refuses to overwrite existing files unless `--force` is used.
pub fn install_action(args: InstallActionArgs) -> Result<()> {
    let workflows_dir = args.repo.join(".github").join("workflows");
    std::fs::create_dir_all(&workflows_dir)
        .with_context(|| format!("creating {}", workflows_dir.display()))?;

    install_workflow(&workflows_dir, "pr-title-check.yml", PR_TITLE_CHECK_YML, args.force)?;
    install_workflow(&workflows_dir, "squash-msg-clean.yml", SQUASH_MSG_CLEAN_YML, args.force)?;

    println!("Installed noa GitHub Actions into {}", workflows_dir.display());
    Ok(())
}

fn install_workflow(dir: &Path, name: &str, content: &str, force: bool) -> Result<()> {
    let path = dir.join(name);
    if path.exists() && !force {
        bail!(
            "{} already exists — use --force to overwrite",
            path.display()
        );
    }
    std::fs::write(&path, content)
        .with_context(|| format!("writing {}", path.display()))?;
    println!("  {name}");
    Ok(())
}

>>>>>>> origin/dev
fn detect_noa_bin() -> Option<String> {
    if let Ok(exe) = std::env::current_exe() {
        return Some(exe.to_string_lossy().to_string());
    }
    None
}

fn which_noa(name: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(':') {
        let candidate = PathBuf::from(dir).join(name);
        if candidate.exists() {
            return Some(candidate.to_string_lossy().to_string());
        }
    }
    None
}
