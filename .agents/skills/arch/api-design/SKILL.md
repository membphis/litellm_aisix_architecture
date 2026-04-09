---
name: arch-api-design
description: AISIX API 接口设计、请求管线、Admin API 和错误格式约定
trigger:
  files:
    - "aisix/crates/aisix-server/src/handlers/**"
    - "aisix/crates/aisix-server/src/pipeline/**"
    - "aisix/crates/aisix-server/src/admin/**"
    - "aisix/crates/aisix-server/src/app.rs"
    - "aisix/crates/aisix-server/src/stream_proxy.rs"
    - "aisix/crates/aisix-auth/**"
    - "aisix/crates/aisix-types/src/error.rs"
    - "aisix/crates/aisix-providers/src/**"
    - "aisix/docs/admin-api.md"
  keywords:
    - "handler"
    - "pipeline"
    - "admin api"
    - "stream chunk"
    - "provider codec"
    - "transport mode"
    - "error response"
    - "openai compatible"
    - "sse"
    - "fallback"
    - "guardrail"
    - "post call"
priority: high
related:
  - arch-style
  - arch-data-model
  - arch-infra
---

# AISIX API 设计与请求管线

按需加载分段，避免全文注入上下文。

## 分段目录

| 分段 | 说明 | 文件 |
|------|------|------|
| 代理 API | 端点列表、OpenAI 兼容原则 | `sections/proxy-api.md` |
| Pipeline | 固定阶段序列、handler 模式、run_pipeline | `sections/pipeline.md` |
| 认证鉴权 | Authentication（Bearer token）+ Authorization（模型白名单） | `sections/auth.md` |
| StreamChunk | SSE/JSON/Binary 流代理、帧边界、fallback、cancel safety | `sections/stream-chunk.md` |
| Provider Codec | ProviderCodec trait、ProviderOutput、复用策略 | `sections/provider-codec.md` |
| PostCall | 异步用量记账、费用追踪、UsageEvent | `sections/post-call.md` |
| Admin API | 路由、认证、写入语义、错误码 | `sections/admin-api.md` |
| 错误格式 | OpenAI 兼容错误 JSON、ErrorKind→HTTP 映射、可重试错误 | `sections/error-format.md` |
| 健康检查 | /health、/ready 端点 | `sections/health.md` |
| 多端点复用 | Pipeline 复用度分析（A/B/C/D 组） | `sections/multi-endpoint.md` |
| 当前妥协 | Phase 1-2 限制 | `sections/compromises.md` |

## 使用方式

根据当前任务需要的主题，用 Read 工具加载对应的 `sections/*.md` 文件，而非全文。
