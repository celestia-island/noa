<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/dev/res/logo/noa.webp" alt="noa" width="200" /></p>

<h1 align="center">Noa</h1>

<p align="center"><strong>面向代理的原生分散式版本控制系統</strong></p>

<div align="center">

[![License](https://img.shields.io/badge/license-SySL%201.0-blue)](../../LICENSE)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fnoa-blue.svg)](https://github.com/celestia-island/noa)

</div>

<div align="center">

[English](../en/intro.md) ·
[簡體中文](../zh-Hans/intro.md) ·
**繁體中文** ·
[日本語](../ja/intro.md) ·
[한국어](../ko/intro.md) ·
[Français](../fr/intro.md) ·
[Español](../es/intro.md) ·
[Русский](../ru/intro.md) ·
[العربية](../ar/intro.md)

</div>

代理各自隔離的工作區、SHA-256 內容定址快照、完整的 git 智慧協定伺服器端（clone/fetch/
push）、自架的鏡像與回流 forge、行級三路合併與精確 rename 偵測、多使用者 RBAC、
CI 執行紀錄 API——全部內建於單一二進位檔 + 內嵌式儲存。

## 文件

完整的繁體中文文件在本儲存庫 `docs/zh-Hant/`（本站同步發布於
[noa.docs.celestia.world](https://noa.docs.celestia.world)）：

| 文件 | 內容 |
|---|---|
| [快速上手](./guide/getting-started.md) | 建置、啟動伺服器端、第一次 clone/push |
| [架構總覽](./guide/architecture.md) | 工作區 / 快照 / 參照模型、伺服器端元件 |
| [鏡像與回流](./guide/mirror.md) | 雙車道、PR heads、回流與 canary、忠實度驗證 |
| [權限與使用者](./guide/rbac.md) | 角色、token、REST 與 git 雙通道執法 |
| [git-http 協定](./guide/git-http.md) | 智慧協定端點、推送語意、協定協商 |
| [合併與衝突](./guide/merge.md) | 行級 diff3、精確 rename、降級語意 |
| [CI 執行紀錄](./guide/ci-runs.md) | runs API、chest 整合 |
| [HTTP API 參考](./reference/http-api.md) | 全部 REST 端點 |
| [CLI 參考](./reference/cli.md) | 命令列全景 |
| [部署](./reference/deploy.md) | systemd、環境變數、部署三件套 |

繁體中文版位於 `docs/zh-Hant/`（自簡體中文逐批同步）。

原始碼：[noa](https://github.com/celestia-island/noa)。
