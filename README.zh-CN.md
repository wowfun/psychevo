<p align="center">
  <img src="assets/psychevo-logo.svg" alt="Psychevo" width="160">
</p>

[English](README.md) | 简体中文

# Psychevo

Psychevo 是一款面向现有代码库的本地编码智能体。配置提供方后，可以从终端、本地 Workbench 或支持的 ACP 编辑器完成任务。配置、权限策略和会话历史都保留在本机，可随时检查。

## 你可以用它做什么

- 按自己的方式工作：使用 `pevo run` 运行一次性任务，使用全屏终端 `UI`，打开本地 Workbench，从源码检出目录启动 Desktop，或连接支持的 ACP 编辑器。
- 选择提供方、模型和智能体：配置兼容 `OpenAI` 的提供方，在可用时使用本地智能体，或在 Workbench 中选择托管的 Codex、OpenCode 和 Hermes ACP 后端。
- 扩展和委派工作流：组合基于文件系统的技能、插件、钩子、`MCP` 工具、本地智能体和子智能体流程。
- 在聊天界面中工作：需要时，可通过管理式 `Gateway` 连接已获授权的 WeChat、Telegram、Feishu 和 Lark 会话。

Psychevo 当前聚焦可靠的本地执行，不宣称已经具备自治评估循环、工作流挖掘或长期记忆能力。

## 从源码安装

Psychevo 当前通过源码安装。先安装 `Git`、`Rust/Cargo`、本机编译器、Node.js 和 pnpm，再运行检出目录中的安装器：

```bash
git clone https://github.com/wowfun/psychevo.git
cd psychevo
sh scripts/install.sh --check
sh scripts/install.sh
```

安装器会构建本地 `pevo` 二进制文件和 Workbench `Web UI` 资源。有关前置条件、诊断、Windows Git Bash 说明、企业网络指导和开发命令，请参阅[安装指南（英文）](docs/install.md)。

如仅需命令行安装，可使用底层 Cargo 命令：

```bash
cargo install --locked --path crates/psychevo-cli --force
```

如需直接开发而不安装：

```bash
cargo run -p psychevo-cli -- --help
pnpm --filter @psychevo/workbench dev
```

## 快速开始

先为提供方和模型完成一次 Psychevo 设置：

```bash
pevo setup
```

向导会初始化 Psychevo 主目录、配置提供方和模型、保存或引用 API 密钥、检查 `Web UI` 资源，并以 doctor 摘要结束。在不联系提供方的情况下确认本地设置：

```bash
pevo doctor
```

在需要处理的项目目录中运行第一个任务：

```bash
pevo run "summarize this repository"
```

需要交互式工作区时：

```bash
pevo tui
pevo web
```

如需编辑器集成，请配置 `ACP` 客户端启动 `pevo acp`。详见[ACP 配置指南（英文）](docs/acp-configuration.md)。

为单次调用选择提供方和模型：

```bash
pevo run -m deepseek/deepseek-chat "inspect the CLI entrypoints"
```

## 文档

- [安装指南（英文）](docs/install.md)
- [ACP 配置指南（英文）](docs/acp-configuration.md)
- [Channels 指南（英文）](docs/channels/README.md)
- [TUI 故障排除（英文）](docs/troubleshooting/tui.md)
- [贡献指南（英文）](CONTRIBUTING.md)

## 更多使用方式

| 当你需要…… | 可以用 Psychevo…… |
|------------|-------------------|
| 从终端工作 | 使用 `pevo run` 运行编码智能体任务，并在选定的当前工作目录中使用本地工具。 |
| 保持在交互式工作区中 | 使用 `pevo tui`、`pevo web`，或从源码检出目录使用 `pevo desktop`。 |
| 使用编辑器或兼容智能体 | 为支持 ACP 的编辑器运行 `pevo acp` 桥接，或在 Workbench 中选择可用的托管 Codex、OpenCode 或 Hermes ACP 智能体。 |
| 管理本地模型和历史 | 使用 Profile、提供方与认证命令，以及记录用量、预估成本和执行证据的本地 `SQLite` 会话。 |
| 扩展或委派任务 | 管理技能、插件、钩子、工具集、本地智能体和子智能体流程。 |
| 提供工具或连接聊天 | 运行仅回环的 `Gateway API`、公开 `MCP` 标准输入输出服务器，或通过管理式 `Gateway` 配置已获授权的 Channels。 |
| 检查准备状态 | 运行 `pevo doctor`，除非明确请求，否则不会执行实时提供方调用。 |

## 命令

| 命令 | 用途 |
|------|------|
| `pevo init` | 创建或修复当前 Psychevo Profile 主目录、初始配置、`.env` 模板和 `SQLite` 状态。 |
| `pevo setup` | 运行交互式首次设置向导，并以本地诊断结束。 |
| `pevo doctor` | 运行确定性的本地诊断。仅在需要提供方网络检查时使用 `--live`。 |
| `pevo run [message..]` | 从终端运行一次编码智能体任务。 |
| `pevo tui [message..]` | 启动全屏终端 `UI`，或逐行处理脚本化标准输入。 |
| `pevo web` | 为当前工作目录打开管理式本地 Workbench `Web UI`。 |
| `pevo desktop` | 从源码检出目录打开原生 Desktop 应用。 |
| `pevo gateway ...` | 打开、启动、检查、停止或重启管理式本地 `Gateway Web Shell`。 |
| `pevo serve` | 在仅回环地址运行严格的无头本地 `Gateway API` 服务器。 |
| `pevo acp` | 为编辑器客户端启动 Agent Client Protocol 标准输入输出服务器。 |
| `pevo mcp serve` | 启动 Model Context Protocol 标准输入输出服务器。 |
| `pevo profile ...` | 列出、检查、创建、切换、别名、重命名和删除本地 Profile。 |
| `pevo agent ...` | 列出、检查、运行和管理本地智能体。 |
| `pevo skill ...` | 列出、查看、创建、安装、配置、审计和管理本地技能与技能包。 |
| `pevo plugin ...` | 列出、检查、安装和启用本地插件。 |
| `pevo hooks ...` | 列出、信任、启用和禁用本地钩子。 |
| `pevo tool ...` | 列出和配置本地工具集。 |
| `pevo session ...` | 列出、检查、重命名、归档、恢复、导出或本地分享会话。 |
| `pevo model ...` | 检查已配置模型，并显式获取提供方模型目录。 |
| `pevo config ...` | 检查配置路径，并添加兼容 `OpenAI` 的提供方。 |
| `pevo auth ...` | 检查凭据状态、运行提供方设置，并保存提供方 API 密钥。 |
| `pevo stats` | 显示本地 `SQLite` 状态中的 token 用量和预估成本统计。 |
| `pevo context --session <id\|latest>` | 检查本地会话的上下文窗口用量。 |

运行 `pevo <command> --help` 查看参数和子命令。

## 开发

修改项目前，请阅读[AGENTS.md](AGENTS.md)。Psychevo 使用 spec-first 原则：在更改行为、公开文档、测试或开发流程之前，先阅读并更新最贴切的 `specs/<topic>/spec.md`。

`Rust` 工作区的广泛验证：

```bash
cargo xtask ci run --profile rust-broad
```

当更窄范围的验证能够覆盖改动行为时，应优先使用它。实际提供方、API 密钥和实时服务验证均需显式选择。

仓库本地的 live 验证由 `xtask` 管理：

```bash
cargo xtask init dev-env
cargo xtask live run
cargo xtask live run --env isolated
cargo xtask live run --suite provider
```

常用本地命令：

```bash
cargo run -p psychevo-cli -- --help
cargo test -p psychevo-cli smoke_cli
pnpm --filter @psychevo/workbench build
pnpm --filter @psychevo/workbench dev
```

## 工作区（贡献者）

| 组件 | 职责 |
|------|------|
| `psychevo-ai` | 提供方协议规范化和 AI 传输适配器。 |
| `psychevo-agent-core` | 模型无关的智能体循环、工具特征、工具执行钩子、结果和中止处理。 |
| `psychevo-runtime` | 编码智能体运行时装配、提供方和模型解析、上下文、工具、持久化、技能、智能体、权限和用量统计。 |
| `psychevo-gateway` | 供 `Web` 和命令行界面使用的本地 `Gateway API` 与 WebSocket 服务器。 |
| `psychevo-acp` | 由 `pevo acp` 使用的 ACP 服务器封装和运行时桥接。 |
| `psychevo-cli` | `pevo` 命令行入口、终端 `UI`、管理式 Web/Gateway 命令、设置和诊断。 |
| `apps/workbench` | 由管理式 `pevo web` 流程提供的 Vite/React Workbench `Web UI`。 |
| `packages/*` | 共享的 TypeScript 协议、客户端、宿主、组件和资源包。 |

## 许可证

Psychevo 使用 [MIT License](LICENSE) 授权。
