# AI 智能体标识与提交共同作者策略

## 概述

`noa` 提供提交时机制，为 AI 生成的提交打上**溯源元数据**：哪些模型产出了改动、通过哪个
提供商触达、消耗了多少 token、以及该改动是否在自主（YOLO）迭代下产出。

该机制实现为一个由 `noa` 安装并解析的 git `commit-msg` 钩子。这是**务实的元数据** ——
为人类提供溯源能力，而非法律闸门。

## 提供商标识模型

作者邮箱使用单一信任命名空间 —— `celestia.world` —— 本地部分编码**谁提供了该模型**：

```
显示名 <provider-or-platform-id@celestia.world>
```

- **第一方**：`anthropic.com`、`deepseek.com`、`openai.com`、`zhipuai.cn`……
- **第三方 / 中转**：`opencode.ai`、`jdcloud.com`、`openrouter.ai`……

同一模型经由不同路径触达时可区分：

```
GLM 5 <zhipuai.cn@celestia.world>               # 直连
GLM 5 <jdcloud.com@celestia.world>            # 经京东云
Deepseek V4 Pro <deepseek.com@celestia.world>  # 直连
Deepseek V4 Pro <opencode.ai@celestia.world>   # 经 opencode
```

## 共同作者 Trailer 规范

- Trailer 键：`Co-authored-by`（git 识别）。
- 值：`显示名 <local@celestia.world>`。
- 每个不同模型一行，按使用顺序排列。

## YOLO 权威 Trailer

当整个思考链完全在 **YOLO 巡航控制**下运行时，会额外前置一条共同作者：

```
Co-authored-by: Entelecheia <demiurge@celestia.world>
```

检测来源：会话聊天日志（`YOLO cruise control` / `YOLO auto` 标记）或
`/run/entelecheia/yolo_active` 哨兵文件。

## 内嵌 Token 用量

追加在共同作者 trailer 之后（空行分隔）：

```
Co-authored-by: Claude Opus 4.8 (↑ 12.5k ↓ 8.3k ⚡45.2k) <anthropic.com@celestia.world>
Co-authored-by: Deepseek V4 Pro (↑ 5.1k ↓ 3.2k) <deepseek.com@celestia.world>
```

- `Upload` = 输入 token；`Download` = 输出 token。
- `Cache` 仅在缓存输入 token 被上报且 > 0 时才出现。
- 计数以千为单位（`k`），保留一位小数，去除尾部零。

## noa 中的实现

### 模块布局

```
src/coauthor/
  mod.rs        — CoAuthor / ModelUsage / CoAuthorReport 类型与渲染
  provider.rs   — 提供商注册表 + endpoint/model → 身份解析
  session.rs    — 聊天日志解析 + 会话聚合 + YOLO 检测
src/cli/
  coauthor_cmd.rs — `noa co-author resolve`
  hook_cmd.rs     — `noa hook install`
assets/
  commit-msg.sh   — 钩子模板（嵌入 @NOA_BIN@）
```

### 命令

```text
noa co-author resolve [--repo <path>] [--chat-log-dir <dir>]
                      [--aporia-config <path>] [--lookback-secs <n>]
noa hook install --repo <path> [--force] [--noa-bin <path>]
```

### 解析算法

1. 构建提供商映射：内置注册表与 `aporia.toml` 合并（精确的 model→endpoint→provider）。
2. 解析最近的聊天日志，按模型聚合 token。
3. 检测 YOLO 模式。
4. 渲染：`Entelecheia` 权威在前（若 YOLO），之后每个模型一条共同作者，再接可选的
   `Token usage` 块。

### 钩子契约

- 写入 `.git/hooks/commit-msg`（权限 `0755`）。
- 调用 `<noa> co-author resolve`，将标准输出追加到提交消息文件。
- **永不阻塞**提交 —— 任何失败都以 `0` 静默退出。
- 若消息已包含 `Co-authored-by:` 则为空操作。
- `NOA_COAUTHOR_DISABLE=1` 可对单次提交禁用钩子。

## 完整提交消息示例

```
fix(auto_fix): raise clippy/check timeouts from 180s to 300s

The previous 180s timeout was too tight; raise it to 300s.

Co-authored-by: Entelecheia <demiurge@celestia.world>
Co-authored-by: GLM 5 (↑ 36.4k ↓ 1.5k) <zhipuai.cn@celestia.world>
```

## 提供商标识参考（初始注册表）

| 提供商 id | 品牌 | 端点提示 |
| --- | --- | --- |
| `zhipuai.cn` | GLM | `open.bigmodel.cn` |
| `deepseek.com` | Deepseek | `api.deepseek.com` |
| `anthropic.com` | Claude | `api.anthropic.com` |
| `openai.com` | GPT / OpenAI | `api.openai.com` |
| `google.com` | Gemini | `googleapis.com` |
| `dashscope.aliyuncs.com` | Qwen | `dashscope.aliyuncs.com` |
| `moonshot.cn` | Kimi | `api.moonshot.cn` |
| `mistral.ai` | Mistral | `api.mistral.ai` |
| `opencode.ai` | （中转） | `opencode.ai` |
| `jdcloud.com` | （中转） | `jdcloud.com` |
| `openrouter.ai` | （中转） | `openrouter.ai` |

## 安全考量

- 共同作者 trailer 是自报告溯源，非密码学证明。
- 解析器安全降级：缺失聊天日志、缺失 `noa` 或解析错误都产出空块，提交不受影响。
- 提供商标识来自本地 `aporia.toml`，反映用户实际配置的提供商。

## 未来工作

- 签名的共同作者证明（密码学）。
- 由内嵌 token 用量派生的成本估算。
- 通过 noa 快照进行跨仓库智能体活动聚合。
