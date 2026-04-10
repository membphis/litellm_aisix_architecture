# 代理 API 端点

## 核心原则：按端点提供协议兼容入口

代理 API 不再只有单一 OpenAI 兼容入口：

- `/v1/chat/completions` 与 `/v1/embeddings` 接受并返回 OpenAI 兼容 JSON/SSE。
- `/v1/messages` 接受并返回 Anthropic Messages JSON/SSE。

网关在内部把不同客户端协议统一归一化为 canonical request，再按目标 provider 转换为上游协议。

## 端点

| 端点 | Operation | TransportMode |
|------|-----------|---------------|
| `POST /v1/chat/completions` | ChatCompletions | `SseStream` 或 `Json` |
| `POST /v1/messages` | AnthropicMessages | `SseStream` 或 `Json` |
| `POST /v1/embeddings` | Embeddings | 仅 `Json` |

Phase 2+：`POST /v1/images/generations`、Audio 端点、`POST /v1/responses`。
