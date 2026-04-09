---
name: arch-style
description: AISIX Rust 编码风格与约定
trigger:
  files:
    - "aisix/**/*.rs"
    - "aisix/**/Cargo.toml"
  keywords:
    - "rust"
    - "coding style"
    - "naming convention"
    - "error handling"
    - "crate"
    - "trait"
    - "pipeline stage"
priority: high
related:
  - arch-data-model
  - arch-api-design
---

# AISIX Rust 编码风格

## 设计哲学

**可读性优先于性能。** Provider 数量有限且固定（MVP: 6），`Arc<dyn ProviderCodec>` 的
动态分发开销相对于 AI 请求秒级延迟可以忽略不计。优先选择清晰、顺序执行的代码，
而非抽象框架。

**不做过早抽象。** 当前阶段刻意避免 plugin/pipeline 系统。Pipeline 阶段是普通函数，
通过 `?` 实现早返回，按顺序调用。这为未来重构保留了清晰的边界。

## 命名规范

### Crate

- 所有 crate 使用 `aisix-` 前缀：`aisix-types`、`aisix-config`、`aisix-auth`
- `Cargo.toml` 中用连字符，Rust 代码中用下划线：`aisix_types`、`aisix_config`

### 类型

- **PascalCase** 用于类型、结构体、枚举、trait
- 配置结构体用 `Config` 后缀：`RateLimitConfig`、`ProviderConfig`
- 运行时/计算结果用纯名词：`ResolvedLimits`、`ResolvedTarget`
- "类别"枚举用 `Kind` 后缀：`ErrorKind`、`ProviderKind`
- 枚举变体用 PascalCase，包括缩写：`OpenAi`（不是 `OpenAI`）

### 字段

- 全部 **snake_case**：`key_id`、`upstream_model`、`provider_id`
- HashMap 查找字段用 `_by_` 模式：`keys_by_token`、`models_by_name`、`providers_by_id`
- ID 字段用 `_id` 后缀：`key_id`、`provider_id`、`policy_id`
- 可选字段直接用 `Option<T>`，不加 `opt_` 前缀：`expires_at: Option<DateTime<Utc>>`

### 函数

- **snake_case**：`compile_snapshot`、`resolve_limits`、`bearer_token`
- 构造函数：`new()` 为主构造器，`with_*` 为变体
- 返回错误的辅助函数用名词/谓语命名：`invalid_api_key() -> GatewayError`

## Import 组织

按以下顺序分组，组间用空行分隔：

```rust
// 1. std
use std::collections::HashMap;
use std::sync::Arc;

// 2. 外部 crate（按字母序）
use axum::{extract::State, response::Response, Json};
use serde::{Deserialize, Serialize};

// 3. aisix_* 内部 crate
use aisix_types::{entities::KeyMeta, request::CanonicalRequest};
use aisix_config::snapshot::CompiledSnapshot;

// 4. 当前 crate
use crate::{app::ServerState, pipeline::authorization};
```

## 错误处理

按层级使用三种不同策略：

### 1. GatewayError（HTTP 边界，`aisix-types::error`）

扁平结构体，不嵌套，不带 backtrace：

```rust
enum ErrorKind { Authentication, Permission, NotFound, InvalidRequest, RateLimited, Timeout, Upstream, Internal }
struct GatewayError { kind: ErrorKind, message: String }
```

- 用结构体字面量显式构造，不通过 `From` 转换
- 实现 `axum::IntoResponse`，输出 OpenAI 兼容 JSON
- 所有 pipeline 阶段返回 `Result<_, GatewayError>`

### 2. RedisError（基础设施层，`aisix-storage`）

使用 `thiserror`，通过 `#[from]` 自动转换 `std::io::Error`。

### 3. Result<_, String>（配置编译）

`compile_snapshot()` 返回 `Result<SnapshotCompileReport, String>`。
错误是格式化字符串，不跨越 HTTP 边界。
编译报告字段和 skip/fail 语义以 `arch-data-model` 为准。

### 规则

- **领域代码中不使用 `anyhow`。** 仅在二进制入口的启动阶段使用。
- 绝不在 HTTP 响应中暴露内部错误细节。在 handler 层映射为 `GatewayError`。

## Crate 依赖层级

```
L0 基础层：     aisix-types              （无内部依赖）
L1 核心设施：   aisix-config, aisix-storage, aisix-core
L2 领域层：     aisix-auth, aisix-policy, aisix-router,
               aisix-ratelimit, aisix-cache, aisix-providers,
               aisix-spend, aisix-observability
L3 编排层：     aisix-runtime, aisix-server
L4 入口层：     aisix-gateway
```

依赖流必须严格自顶向下。L0 不能依赖 L1+。
L2 可以依赖 L0-L1，但 L2 之间不应互相依赖（除非有明确理由）。

## 序列化

- 所有配置/数据类型同时派生 `Serialize, Deserialize`
- `#[serde(rename = "...")]` 用于线上格式的名称映射和 Rust 保留字
- `#[serde(default)]` 用于向后兼容的可选字段
- `GatewayError` 手动构造 JSON 响应（不派生 `Serialize`）

## Trait 设计

- `ProviderCodec`：`#[async_trait]` + `Send + Sync + 'static` 约束
- 私有辅助 trait（如 `HasConfigId`）用于泛型集合逻辑
- Axum extractor 通过 `FromRequestParts` 实现，用 `FromRef` 链式提取状态

## 并发模式

- `ArcSwap<CompiledSnapshot>` 实现无锁配置读取 + 原子交换
- 所有跨 async 边界的结构体派生 `Clone`
- RAII guard 用于资源清理（并发租约、连接）
- 所有 async 代码必须 cancel-safe：每个 `await` 点都可能被取消，不能有状态泄漏

## Pipeline 阶段约定

每个阶段是 `src/pipeline/` 下的独立文件：
```
pipeline/
├── context.rs          # RequestContext 定义
├── authorization.rs
├── rate_limit.rs
├── cache.rs
├── route_select.rs
├── stream_chunk.rs
└── post_call.rs
```

- 阶段是自由函数，不是 trait object
- 每个阶段通过 `key_id` 查询编译快照；无配置 = no-op
- `RequestContext` 以 `&mut` 传递，逐步累积状态
- `PostCall` 在 `tokio::spawn` 中运行，接收 owned 的 `PostCallContext`（不借用 ctx）

## 测试

- 集成测试放在 `crates/*/tests/`，使用 `#[tokio::test]`
- 描述性 snake_case 命名：`maps_too_many_requests_to_rate_limit_error`
- 直接构造类型，不使用测试 fixture 或 builder
- 共享 `TestApp::start()` 辅助函数用于全栈测试
- 外部依赖通过 wiremock + docker-compose 提供

## 当前阶段妥协

- [!NOTE] 没有 plugin/pipeline 系统。预计 1-2 个季度后根据生产经验重新评估。
- [!NOTE] 自定义最小化 Redis 客户端（原始 RESP 协议）。无连接池、无 TLS。
  这避免了重量级依赖，但功能受限。Phase 2 可能升级到 `redis` crate。
- [!NOTE] 公开项尚未添加 `///` 文档注释。外部文档仅存在于 `docs/` 目录。
