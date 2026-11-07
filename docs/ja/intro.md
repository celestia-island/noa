<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/dev/res/logo/noa.webp" alt="noa" width="200" /></p>

<h1 align="center">Noa</h1>

<p align="center"><strong>AIネイティブな分散バージョン管理システム</strong></p>

<div align="center">

[![License](https://img.shields.io/badge/license-SySL%201.0-blue)](../../LICENSE)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fnoa-blue.svg)](https://github.com/celestia-island/noa)

</div>

<div align="center">

[English](../en/intro.md) ·
[简体中文](../zh-Hans/intro.md) ·
[繁體中文](../zh-Hant/intro.md) ·
**日本語** ·
[한국어](../ko/intro.md) ·
[Français](../fr/intro.md) ·
[Español](../es/intro.md) ·
[Русский](../ru/intro.md) ·
[العربية](../ar/intro.md)

</div>

エージェントごとのワークスペース隔離、SHA-256 内容アドレス指定スナップショット、完全な git スマート HTTP サーバー（clone/fetch/**push**）、ミラーと還流レーンを内蔵したセルフホストフォージ、同一内容リネーム検出つきの行レベル 3ウェイマージ、マルチユーザー RBAC、CI 実行記録 API——すべて単一バイナリ + 埋め込みストレージに収まっています。

## ドキュメント

ドキュメント一式はこのリポジトリに含まれています（[noa.docs.celestia.world](https://noa.docs.celestia.world) でも公開）。**简体中文（簡体字中国語）が原文（source of truth）**で、その他の言語はそこから同期されます。

| ドキュメント | 内容 |
|---|---|
| [クイックスタート](./guide/getting-started.md) | ビルド、サーバーの起動、最初の clone/push |
| [アーキテクチャ](./guide/architecture.md) | ワークスペース / スナップショット / 参照モデル |
| [ミラーと還流](./guide/mirror.md) | 二つのレーン、PR head、還流、カナリア |
| [権限とユーザー / RBAC](./guide/rbac.md) | ロール、トークン、権限の強制 |
| [git-http プロトコル](./guide/git-http.md) | スマートプロトコルのエンドポイント、push のセマンティクス |
| [マージと競合](./guide/merge.md) | 行レベル diff3、同一内容リネーム、縮退時の動作 |
| [CI 実行記録](./guide/ci-runs.md) | 実行記録、chest との統合 |
| [HTTP API リファレンス](./reference/http-api.md) | すべての REST エンドポイント |
| [CLI リファレンス](./reference/cli.md) | コマンド一覧 |
| [デプロイ](./reference/deploy.md) | systemd、環境変数、ロールアウト |

ソース: [noa](https://github.com/celestia-island/noa)。
