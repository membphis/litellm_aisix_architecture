# PostCall（异步、不阻塞响应）

- 在 `tokio::spawn` 中运行 — 不阻塞响应
- 接收 owned 的 `PostCallContext`（String、u64、bool — 不借用 ctx）
- 职责：Redis 用量记账、费用追踪、结构化日志
- 通过 structlog + callback sink 输出 `UsageEvent`
