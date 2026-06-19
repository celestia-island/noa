# AI Agent Identification & Commit Co-author Strategy

## Overview

`noa` provides the commit-time mechanism that stamps AI-generated commits with
**provenance metadata**: which models authored a change, through which provider they
were reached, how many tokens they consumed, and whether the change was produced
under autonomous (YOLO) iteration.

This is implemented as a git `commit-msg` hook that `noa` installs and resolves.
The mechanism is **pragmatic metadata** — traceability for humans, not a legal gate.

## Provider Identity Model

The author email uses a single trust namespace — `celestia.world` — with the local
part encoding **who served the model**:

```
Display Name <provider-or-platform-id@celestia.world>
```

- **First-party**: `anthropic.com`, `deepseek.com`, `openai.com`, `zhipuai.cn`, ...
- **Third-party / relay**: `opencode.ai`, `jdcloud.com`, `openrouter.ai`, ...

The same model reached through different routes is distinguishable:

```
GLM 5 <zhipuai.cn@celestia.world>               # direct
GLM 5 <jdcloud.com@celestia.world>            # via JD Cloud
Deepseek V4 Pro <deepseek.com@celestia.world>  # direct
Deepseek V4 Pro <opencode.ai@celestia.world>   # via opencode
```

## Co-author Trailer Specification

- Trailer key: `Co-authored-by` (git-recognised).
- Value: `Display Name <local@celestia.world>`.
- One trailer per distinct model, in usage order.

## YOLO Authority Trailer

When the chain of thought ran entirely under **YOLO cruise control**, an additional
co-author is prepended:

```
Co-authored-by: Entelecheia <demiurge@celestia.world>
```

Detection sources: the session chat log (`YOLO cruise control` / `YOLO auto`
marker) or the `/run/entelecheia/yolo_active` sentinel file.

## Embedded Token Usage

Token usage is embedded directly inside each model's display name within the
`Co-authored-by` trailer, so the entire provenance stays a single trailer block
that GitHub parses correctly (a trailing free-form block would break trailer
parsing):

```
Co-authored-by: Claude Opus 4.8 (↑ 12.5k ↓ 8.3k ⚡45.2k) <anthropic.com@celestia.world>
Co-authored-by: Deepseek V4 Pro (↑ 5.1k ↓ 3.2k) <deepseek.com@celestia.world>
```

- `↑` = input/upload tokens; `↓` = output/download tokens.
- `⚡` (cache) appears **only** when cached-input tokens were reported and are > 0.
- Counts in thousands (`k`), one decimal place, trailing-zero trimmed.

## Implementation in noa

### Module layout

```
src/coauthor/
  mod.rs        — CoAuthor / ModelUsage / CoAuthorReport types + rendering
  provider.rs   — provider registry + endpoint/model → identity resolution
  session.rs    — chat-log parsing + session aggregation + YOLO detection
src/cli/
  coauthor_cmd.rs — `noa co-author resolve`
  hook_cmd.rs     — `noa hook install`
assets/
  commit-msg.sh   — the hook template (embeds @NOA_BIN@)
```

### Commands

```text
noa co-author resolve [--repo <path>] [--chat-log-dir <dir>]
                      [--aporia-config <path>] [--lookback-secs <n>]
noa hook install --repo <path> [--force] [--noa-bin <path>]
```

### Resolution algorithm

1. Build the provider map: built-in registry merged with `aporia.toml`
   (precise model→endpoint→provider mapping).
2. Parse the most recent chat log(s); aggregate tokens per model.
3. Detect YOLO mode.
4. Render: `Entelecheia` authority first (if YOLO), then one co-author per model
   with token usage embedded in the display name.

### Hook contract

- Writes `.git/hooks/commit-msg` (mode `0755`).
- Calls `<noa> co-author resolve`, appends stdout to the commit message file.
- **Never blocks** a commit — on any failure it exits `0` silently.
- No-op if the message already contains `Co-authored-by:`.
- `NOA_COAUTHOR_DISABLE=1` disables the hook for one commit.

## Full Commit Message Example

```
fix(auto_fix): raise clippy/check timeouts from 180s to 300s

The previous 180s timeout was too tight; raise it to 300s.

Co-authored-by: Entelecheia <demiurge@celestia.world>
Co-authored-by: GLM 5 (↑ 36.4k ↓ 1.5k) <zhipuai.cn@celestia.world>
```

## Provider Identifier Reference (initial registry)

| Provider id | Brand | Endpoint hint |
| --- | --- | --- |
| `zhipuai.cn` | GLM | `open.bigmodel.cn` |
| `deepseek.com` | Deepseek | `api.deepseek.com` |
| `anthropic.com` | Claude | `api.anthropic.com` |
| `openai.com` | GPT / OpenAI | `api.openai.com` |
| `google.com` | Gemini | `googleapis.com` |
| `dashscope.aliyuncs.com` | Qwen | `dashscope.aliyuncs.com` |
| `moonshot.cn` | Kimi | `api.moonshot.cn` |
| `mistral.ai` | Mistral | `api.mistral.ai` |
| `opencode.ai` | (relay) | `opencode.ai` |
| `jdcloud.com` | (relay) | `jdcloud.com` |
| `openrouter.ai` | (relay) | `openrouter.ai` |

## Security Considerations

- Co-author trailers are self-reported provenance, not cryptographic proof.
- The resolver degrades safely: a missing chat log, missing `noa`, or a parse error
  all yield an empty block and the commit proceeds untouched.
- Provider identifiers come from the local `aporia.toml`, reflecting the providers
  the user actually configured.

## Future Work

- Signed co-author attestations (cryptographic).
- Cost estimation derived from the token-usage block.
- Cross-repository agent-activity aggregation via noa snapshots.
