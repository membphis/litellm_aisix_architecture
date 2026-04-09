# 并发模式

- `ArcSwap<CompiledSnapshot>` 实现无锁配置读取 + 原子交换
- 所有跨 async 边界的结构体派生 `Clone`
- RAII guard 用于资源清理（并发租约、连接）
- 所有 async 代码必须 cancel-safe：每个 `await` 点都可能被取消，不能有状态泄漏
