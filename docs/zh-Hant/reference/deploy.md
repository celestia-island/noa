# 部署

noa-server 的部署形態：單二进制 + 單 redb 資料庫 + systemd 單元。生产實例由 malkuth
监管。

## 部署三件套

1. **产物**：`cargo build --release --locked --bin noa-server` 的产物拷贝到部署路径
   （原子替换：先寫 `<bin>.new` 再 rename），雙侧 sha256 核对；
2. **health 重试**：啟动後輪询 `/api/v1/*`（401 = 进程活且鑑權工作）；
3. **回滚**：保留上一版产物路径，`mv` 回退 + 重啟。

## 環境變數（A 類引導設定 → 0600 env 文件）

| 變量 | 说明 |
|---|---|
| `NOA_API_TOKEN` | 引導 admin token；**空/全空白拒绝啟动** |
| `NOA_MIRROR_ENABLED` | 鏡像調度開關 |
| `NOA_MIRROR_INTERVAL_SECS` | 波次间隔（預設 900） |
| `NOA_MIRROR_SCRATCH_DIR` | scratch 根目錄（如 `/var/lib/noa/mirror-cache`） |
| `NOA_MIRROR_GH_TOKEN` | 回流 / canary 的 GitHub 寫憑證（token_env 預設名） |
| `NOA_MIRROR_GH_TOKEN_ENV` | 覆盖憑證的環境變數名（預設即上者） |

憑證只进部署面（0600 env / systemd LoadCredential），永不进 DB、git 树或文檔。

## systemd 單元

```ini
[Service]
EnvironmentFile=/etc/noa/noa-server.env
ExecStart=/usr/local/bin/noa-server --host 0.0.0.0 --port 3016
Restart=on-failure
```

- 监听 `0.0.0.0`（家族節點惯例：全路由 bearer + 可寫角色執法，暴露面安全）；
- 改 env 後 `systemctl daemon-reload && systemctl restart`；驗證
  `tr '\0' '\n' < /proc/$MAINPID/environ` 含新鍵；
- 生产實例由 malkuth 监管（改动部署路径前先走认领独占）。

## 端口與域名

端口分配见 workspace 帳本（`_ledger/ports-and-domains.md`）；生产實例 3016，nginx
前门在其上做 TLS 終结。

## 多實例

redb 是單文件庫——一个實例一个庫文件。多實例 = 多目錄多端口（如 `chest-demo` 與
生产 chest 的雙實例模式）；跨實例設定搬运走 RPC import / export，不拷設定文件。

## 部署後核驗

```bash
# 鑑權與就绪
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3016/api/v1/prs   # 期望 401
# 鏡像波次
curl -X POST -H "Authorization: Bearer $T" -H "Content-Type: application/json" \
  -d '{"job":"noa"}' http://127.0.0.1:3016/api/v1/mirror/run               # 期望 202
```

鏡像細節與回流語义见[鏡像與回流](../guide/mirror.md)。
