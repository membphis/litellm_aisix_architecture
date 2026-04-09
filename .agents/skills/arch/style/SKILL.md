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

按需加载分段，避免全文注入上下文。

## 分段目录

| 分段 | 说明 | 文件 |
|------|------|------|
| 设计哲学 | 可读性优先、不做过早抽象 | `sections/philosophy.md` |
| 命名规范 | crate / 类型 / 字段 / 函数命名约定 | `sections/naming.md` |
| Import 组织 | 四组顺序规则 | `sections/imports.md` |
| 错误处理 | GatewayError / RedisError / Result<_, String> 三层策略 | `sections/error-handling.md` |
| Crate 依赖层级 | L0-L4 依赖流规则 | `sections/crate-layers.md` |
| 序列化 | Serde 约定 | `sections/serialization.md` |
| Trait 设计 | ProviderCodec、HasConfigId、Axum extractor | `sections/traits.md` |
| 并发模式 | ArcSwap、RAII guard、cancel safety | `sections/concurrency.md` |
| Pipeline 阶段 | 目录结构、阶段约定 | `sections/pipeline.md` |
| 测试 | 集成测试约定 | `sections/testing.md` |
| 当前妥协 | Phase 1 限制 | `sections/compromises.md` |

## 使用方式

根据当前任务需要的主题，用 Read 工具加载对应的 `sections/*.md` 文件，而非全文。
