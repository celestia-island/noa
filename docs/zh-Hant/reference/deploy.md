# 部署

noa-server 的部署形態：單一執行檔 + 單一 redb 資料庫 + systemd 單元。正式環境實例由 malkuth
監管。

## 部署三件套

1. **產物**：`cargo build --release --locked --bin noa-server` 的產物複製到部署路徑
   （原子替換：先寫 `<bin>.new` 再 rename），雙側以 sha256 核對；
2. **health 重試**：啟動後輪詢 `/api/v1/*`（401 = 行程存活且驗證正常運作）；
3. **回滾**：保留上一版產物路徑，以 `mv` 回退 + 重新啟動。

## 環境變數（A 類引導設定 → 0600 env 檔案）

| 變數 | 說明 |
|---|---|
| `NOA_API_TOKEN` | 引導 admin token；**空值/全空白拒絕啟動** |
| `NOA_MIRROR_ENABLED` | 鏡像排程開關 |
| `NOA_MIRROR_INTERVAL_SECS` | 波次間隔（預設 900） |
| `NOA_MIRROR_SCRATCH_DIR` | scratch 根目錄（如 `/var/lib/noa/mirror-cache`） |
| `NOA_MIRROR_GH_TOKEN` | 回流 / canary 的 GitHub 寫入憑證（token_env 預設名） |
| `NOA_MIRROR_GH_TOKEN_ENV` | 覆寫憑證的環境變數名（預設即上者） |

憑證只進部署面（0600 env / systemd LoadCredential），永不進 DB、git 樹或文件。

## systemd 單元

```ini
[Service]
EnvironmentFile=/etc/noa/noa-server.env
ExecStart=/usr/local/bin/noa-server --host 0.0.0.0 --port 3016
Restart=on-failure
```

- 監聽 `0.0.0.0`（家族節點慣例：所有路由皆需 bearer + 可寫角色強制執行，暴露面安全）；
- 改 env 後執行 `systemctl daemon-reload && systemctl restart`；驗證
  `tr '\0' '\n' < /proc/$MAINPID/environ` 含新鍵；
- 正式環境實例由 malkuth 監管（改動部署路徑前先走認領獨佔）。

## 連接埠與網域

連接埠分配見 workspace 帳本（`_ledger/ports-and-domains.md`）；正式環境實例為 3016，nginx
入口在其上做 TLS 終結。

## 多實例

redb 是單檔資料庫——一個實例一個資料庫檔案。多實例 = 多目錄多連接埠（如 `chest-demo` 與
正式環境 chest 的雙實例模式）；跨實例的設定搬運走 RPC import / export，不複製設定檔。

## 部署後核驗

```bash
# 驗證與就緒
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3016/api/v1/prs   # 預期 401
# 鏡像波次
curl -X POST -H "Authorization: Bearer $T" -H "Content-Type: application/json" \
  -d '{"job":"noa"}' http://127.0.0.1:3016/api/v1/mirror/run               # 預期 202
```

鏡像細節與回流語意見[鏡像與回流](../guide/mirror.md)。
