# 代理 API 端点

## 核心原则：OpenAI 兼容接口

所有代理端点接受和返回 OpenAI 兼容的 JSON/SSE 格式。
网关在内部处理不同 Provider 格式之间的转换。

## 端点

| 端点 | Operation | TransportMode |
|------|-----------|---------------|
| `POST /v1/chat/completions` | ChatCompletions | `SseStream` 或 `Json` |
| `POST /v1/embeddings` | Embeddings | 仅 `Json` |

Phase 2+：`POST /v1/images/generations`、Audio 端点、`POST /v1/responses`。
