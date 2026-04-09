# 核心架构原则

**不可变编译快照 + ArcSwap 原子切换。** 配置热加载零停机。
watcher 从 etcd 编译新快照后原子替换：依赖无效的当前资源会被跳过并视为 absent，硬错误则阻止发布并保留旧快照。

```
etcd watch → 后台 tokio 任务 → compile_snapshot() → ArcSwap.store()
```
