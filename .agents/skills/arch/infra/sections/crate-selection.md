# Crate 选型

| 用途 | Crate | 备注 |
|------|-------|------|
| HTTP 服务 | `axum` + `tower` | Tower 中间件处理横切关注点 |
| HTTP 客户端 | `reqwest` | 上游 Provider 调用，连接池 |
| 异步运行时 | `tokio` | 多线程调度器 |
| 序列化 | `serde` + `serde_json` + `serde_yaml` | |
| etcd 客户端 | `etcd-client` | |
| Redis | 自定义 RESP 客户端 | Phase 1 最小化；Phase 2 可能升级 |
| 限流 | `governor` | GCRA 算法用于影子限流器 |
| 指标 | `prometheus` | |
| 链路追踪 | `tracing` + `tracing-subscriber` | |
| 原子配置 | `arc-swap` | 无锁快照交换 |
| 时间戳 | `chrono` | |
| 字节 | `bytes` | 热路径零拷贝 |
| UUID | `uuid` | 请求 ID |
| 错误派生 | `thiserror` | 仅基础设施错误 |

## 最小化依赖哲学

12 个共享 workspace 依赖。不使用 `redis` crate（自定义客户端）。
领域代码中不使用 `anyhow`。无插件框架。
