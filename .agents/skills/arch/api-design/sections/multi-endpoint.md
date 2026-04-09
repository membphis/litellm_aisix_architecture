# 多端点 Pipeline 复用

| 组别 | API | 复用度 | 备注 |
|------|-----|-------|------|
| A | Chat、Responses | ~90% | SSE + 非流式 |
| B | Embeddings、Images | ~70% | 仅 JSON |
| C | Audio | ~55% | Multipart/二进制 |
| D | Realtime、MCP | ~20-30% | 不同协议 |

A+B 组共享 `run_pipeline()`。差异通过 `CanonicalRequest` 枚举和
`ProviderCodec` trait 多态消化。
