# Pipeline 阶段约定

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
