# NodeGet 开发者参考文档（CONTRIBUTING/）

本目录是 NodeGet 后端（Rust workspace）的**开发者参考**，由逐行阅读全部源码（250 个 `.rs` 文件、约 44k 行）后综合而成。文档以源码为依据，覆盖逐 crate 参考、横向机制与维护约定；其中很多章节带有 `文件:行号` 锚点，但锚点覆盖率并非对所有文档、所有条目都完全一致。

> 维护者**必须**在本目录文档的指导下进行大型修改。详见 `.claude/contributing-enforcement.md`。

## 目录结构

```
CONTRIBUTING/
├── README.md                     # 本文件：导航索引
├── CONTRIBUTING.md               # 贡献流程、Issue/PR 规范、代码风格（原根 CONTRIBUTING.md）
├── topics/
│   ├── architecture.md           # 系统架构、通信协议、数据流、启动/热重载生命周期
│   ├── conventions.md            # 编码规范执行手册：错误处理、RPC 四层结构、缓存、serde、日志
│   └── cross-cutting.md          # 跨 crate 机制：Trait 注入、RBAC 权限模型、缓存框架、安全边界
├── crates/                       # 每个 crate 一份详尽参考
│   ├── ng-core.md
│   ├── ng-db.md
│   ├── ng-infra.md
│   ├── ng-config.md
│   ├── ng-monitoring.md
│   ├── ng-token.md
│   ├── ng-kv.md
│   ├── ng-task.md
│   ├── ng-crontab.md
│   ├── ng-js-runtime.md
│   ├── ng-js-worker.md
│   ├── ng-static.md
│   └── ng-terminal.md
└── binaries/                     # 两个二进制的入口与生命周期
    ├── nodeget-server.md
    └── nodeget-agent.md
```

## 如何使用本目录

### 我要…

| 场景 | 先读 |
|------|------|
| 理解整体架构、请求如何流转 | [`topics/architecture.md`](topics/architecture.md) |
| 新增/修改 RPC 方法、想知道编码规范 | [`topics/conventions.md`](topics/conventions.md) + 对应 [`crates/<name>.md`](crates/) |
| 理解权限模型、缓存框架、Trait 注入 | [`topics/cross-cutting.md`](topics/cross-cutting.md) |
| 改某个 crate 的内部实现 | [`crates/<name>.md`](crates/) 的「内部机制」「注意事项与陷阱」 |
| 改 server/agent 启动或接线 | [`binaries/nodeget-server.md`](binaries/nodeget-server.md) / [`binaries/nodeget-agent.md`](binaries/nodeget-agent.md) |
| 提 Issue / PR、代码风格 | [`CONTRIBUTING.md`](CONTRIBUTING.md) |

### 每个 crate / binary 文档的统一结构

1. **概览**（blockquote）— crate 职责一句话
2. **模块结构** — 源文件树与各自角色
3. **公共 API** / **入口与启动流程**（二进制）
4. **关键类型与常量** — 带 `文件:行号` 锚点
5. **内部机制** — 数据流、并发、缓存生命周期、panic 边界
6. **RPC 方法**（如有）— 命名空间 / 方法 / 参数 / 权限 / 行为
7. **数据库实体 / 迁移**（如有）
8. **Crate 内部约定** — feature 门控、serde 规则、日志 target、命名
9. **注意事项与陷阱** — 维护者必须遵守的不变量，每条带锚点
10. **依赖关系** — 上下游 workspace crate

## 文档与源码的对应保证

- 每份 crate 文档基于该 crate **全部源文件**的逐行阅读产出（第一阶段 deep-read）。
- 关键事实经过独立对抗性校验（第二阶段 verify）：抽样打开真实源码核对 `文件:行号`、签名、数值常量、权限语义，全部判定为「高度可信」。
- `ng-js-worker` 因 deep-read 阶段遭遇临时限流，由人工逐行补读其 16 个文件后单独成文。
- 文档**不是**自动生成的 API 文档；它是面向维护者的参考手册，聚焦行为、不变量与陷阱。

## 文档维护原则

- **源码改动后同步更新对应文档**：改了某 crate 的行为/签名/不变量，必须更新 `crates/<name>.md` 的相应章节，保持锚点准确。
- **大型修改必须先读本目录**：架构级改动（新增 RPC 命名空间、改权限模型、改缓存框架、改启动流程、跨 crate 接口变更）开始前，先读 `topics/` 与相关 `crates/`，避免破坏既有不变量。
- **锚点漂移容忍**：`文件:行号` 会随重构漂移；以**函数/类型名 + 语义**为准，行号仅作导航辅助。若语义与源码冲突，**以源码为准**并订正文档。

## 与其他文档的关系

| 文档 | 受众 | 范围 |
|------|------|------|
| `CONTRIBUTING/`（本目录） | Rust 维护者 | 后端源码逐行参考、架构、规范执行 |
| 根 `CLAUDE.md` | AI 助手 + 维护者 | 架构速览、关键约定、构建命令（高层） |
| `docs/`（VitePress） | 终端用户 + 主题/Worker/扩展开发者 | 面向用户的安装、配置、API、扩展开发 |
| `rp.md` | Rust 开发者 | 技术全解速查（高层） |

本目录与根 `CLAUDE.md` 互补：`CLAUDE.md` 是高密度速览，本目录是展开的逐 crate / 主题参考。两者冲突时，**优先参考本目录中与当前改动直接相关的 `crates/<name>.md`、`binaries/*.md` 与 `topics/*.md`**；若文档与当前源码冲突，则以源码为准并同步订正文档。
