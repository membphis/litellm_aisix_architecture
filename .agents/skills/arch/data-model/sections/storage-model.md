# 四层存储模型

| 层级 | 位置 | 内容 |
|------|------|------|
| L1 热路径 | 进程内 `Arc<CompiledSnapshot>` | 路由索引、策略表、Provider 注册表 |
| L2 分布式计数器 | Redis | RPM/TPM 计数器、并发租约、冷却标记 |
| L3 共享缓存 | Redis / 进程内内存 | 响应缓存（Phase 1: moka/DashMap 进程内） |
| L4 持久化真相 | PostgreSQL（仅控制面） | 用量账本、审计日志、预算定义 |
