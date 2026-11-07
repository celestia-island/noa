<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/dev/res/logo/noa.webp" alt="noa" width="200" /></p>

<h1 align="center">Noa</h1>

<p align="center"><strong>面向智能体的原生分布式版本控制系统</strong></p>

<div align="center">

[![License](https://img.shields.io/badge/license-SySL%201.0-blue)](../../LICENSE)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fnoa-blue.svg)](https://github.com/celestia-island/noa)

</div>

<div align="center">

[English](../en/intro.md) ·
**简体中文** ·
[繁體中文](../zh-Hant/intro.md) ·
[日本語](../ja/intro.md) ·
[한국어](../ko/intro.md) ·
[Français](../fr/intro.md) ·
[Español](../es/intro.md) ·
[Русский](../ru/intro.md) ·
[العربية](../ar/intro.md)

</div>

按智能体隔离的工作区、SHA-256 内容寻址快照、完整的 git 智能协议服务端（clone/fetch/
push）、自带镜像与回流的自托管 forge、行级三路合并与精确 rename 检测、多用户 RBAC、
CI 运行记录 API——全部内置于单二进制 + 嵌入式存储。

## 文档

完整的中文文档在本仓 `docs/zh-Hans/`（本站同步发布于
[noa.docs.celestia.world](https://noa.docs.celestia.world)）：

| 文档 | 内容 |
|---|---|
| [快速上手](./guide/getting-started.md) | 构建、启动服务端、第一次 clone/push |
| [架构总览](./guide/architecture.md) | 工作区 / 快照 / 引用模型、服务端组件 |
| [镜像与回流](./guide/mirror.md) | 双车道、PR heads、回流与 canary、忠实度校验 |
| [权限与用户](./guide/rbac.md) | 角色、token、REST 与 git 双通道执法 |
| [git-http 协议](./guide/git-http.md) | 智能协议端点、推送语义、协议协商 |
| [合并与冲突](./guide/merge.md) | 行级 diff3、精确 rename、降级语义 |
| [CI 运行记录](./guide/ci-runs.md) | runs API、chest 集成 |
| [HTTP API 参考](./reference/http-api.md) | 全部 REST 端点 |
| [CLI 参考](./reference/cli.md) | 命令行全景 |
| [部署](./reference/deploy.md) | systemd、环境变量、部署三件套 |

英文版位于 `docs/en/`（自简体中文逐批同步）。

源码：[noa](https://github.com/celestia-island/noa)。
