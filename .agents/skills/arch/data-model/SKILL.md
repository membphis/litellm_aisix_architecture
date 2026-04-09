---
name: arch-data-model
description: AISIX 数据模型约定、etcd 实体 schema 和配置编译规则
trigger:
  files:
    - "aisix/crates/aisix-config/**"
    - "aisix/crates/aisix-types/src/entities.rs"
    - "aisix/crates/aisix-types/src/request.rs"
    - "aisix/crates/aisix-types/src/usage.rs"
    - "aisix/crates/aisix-types/src/stream.rs"
    - "aisix/crates/aisix-core/src/context.rs"
    - "aisix/crates/aisix-policy/**"
  keywords:
    - "etcd"
    - "entity"
    - "snapshot"
    - "compiled snapshot"
    - "data model"
    - "rate limit"
    - "policy"
    - "provider config"
    - "model config"
    - "api key"
    - "request context"
    - "key meta"
    - "usage"
    - "transport mode"
priority: high
related:
  - arch-style
  - arch-api-design
  - arch-infra
---

# AISIX 数据模型与配置系统

按需加载分段，避免全文注入上下文。

## 分段目录

| 分段 | 说明 | 文件 |
|------|------|------|
| 核心原则 | 不可变快照 + ArcSwap、编译-发布流程 | `sections/core-principle.md` |
| etcd Key 布局 | 四类实体的 key 前缀结构 | `sections/etcd-layout.md` |
| 实体 Schema | Policy / Provider / Model / APIKey 的 JSON schema 和字段说明 | `sections/entities.md` |
| 限流解析 | 三层独立检查、内联覆盖语义、Redis key 模式 | `sections/rate-limit.md` |
| 核心 Rust 类型 | CanonicalRequest、TransportMode、StreamEvent、KeyMeta、RequestContext、CompiledSnapshot | `sections/rust-types.md` |
| 配置编译规则 | compile_snapshot() 的校验规则、资源级 skip / 硬错误发布语义 | `sections/compilation.md` |
| 四层存储模型 | L1-L4 存储层级划分 | `sections/storage-model.md` |
| Serde 约定 | 序列化/反序列化规则 | `sections/serde.md` |
| 当前妥协 | Phase 1 限制和后续计划 | `sections/compromises.md` |

## 使用方式

根据当前任务需要的主题，用 Read 工具加载对应的 `sections/*.md` 文件，而非全文。
