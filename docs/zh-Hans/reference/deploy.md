# 部署

noa-server 的部署形态：单二进制 + 单 redb 数据库 + systemd 单元。生产实例由 malkuth
监管。

## 部署三件套

1. **产物**：`cargo build --release --locked --bin noa-server` 的产物拷贝到部署路径
   （原子替换：先写 `<bin>.new` 再 rename），双侧 sha256 核对；
2. **health 重试**：启动后轮询 `/api/v1/*`（401 = 进程活且鉴权工作）；
3. **回滚**：保留上一版产物路径，`mv` 回退 + 重启。

## 环境变量（A 类引导配置 → 0600 env 文件）

| 变量 | 说明 |
|---|---|
| `NOA_API_TOKEN` | 引导 admin token；**空/全空白拒绝启动** |
| `NOA_MIRROR_ENABLED` | 镜像调度开关 |
| `NOA_MIRROR_INTERVAL_SECS` | 波次间隔（默认 900） |
| `NOA_MIRROR_SCRATCH_DIR` | scratch 根目录（如 `/var/lib/noa/mirror-cache`） |
| `NOA_MIRROR_GH_TOKEN` | 回流 / canary 的 GitHub 写凭据（token_env 默认名） |
| `NOA_MIRROR_GH_TOKEN_ENV` | 覆盖凭据的环境变量名（默认即上者） |

凭据只进部署面（0600 env / systemd LoadCredential），永不进 DB、git 树或文档。

## systemd 单元

```ini
[Service]
EnvironmentFile=/etc/noa/noa-server.env
ExecStart=/usr/local/bin/noa-server --host 0.0.0.0 --port 3016
Restart=on-failure
```

- 监听 `0.0.0.0`（家族节点惯例：全路由 bearer + 可写角色执法，暴露面安全）；
- 改 env 后 `systemctl daemon-reload && systemctl restart`；验证
  `tr '\0' '\n' < /proc/$MAINPID/environ` 含新键；
- 生产实例由 malkuth 监管（改动部署路径前先走认领独占）。

## 端口与域名

端口分配见 workspace 账本（`_ledger/ports-and-domains.md`）；生产实例 3016，nginx
前门在其上做 TLS 终结。

## 多实例

redb 是单文件库——一个实例一个库文件。多实例 = 多目录多端口（如 `chest-demo` 与
生产 chest 的双实例模式）；跨实例配置搬运走 RPC import / export，不拷配置文件。

## 部署后核验

```bash
# 鉴权与就绪
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3016/api/v1/prs   # 期望 401
# 镜像波次
curl -X POST -H "Authorization: Bearer $T" -H "Content-Type: application/json" \
  -d '{"job":"noa"}' http://127.0.0.1:3016/api/v1/mirror/run               # 期望 202
```

镜像细节与回流语义见[镜像与回流](../guide/mirror.md)。
