# 热加载机制

## 流程

```
etcd watch 事件 (revision N+1)
  → 防抖窗口（250-500ms）
  → 后台 tokio 任务：重新编译快照
  → ArcSwap.store(new_snapshot)（< 1μs，无锁）
  → 新请求自动使用新快照
  → 进行中的请求持有旧快照（Arc 引用计数）
```

## 安全保障

| 机制 | 说明 |
|------|------|
| 防抖窗口 | 250-500ms 合并窗口，处理快速连续变更 |
| 后台编译 | 不阻塞请求路径 |
| 原子指针交换 | 通过 ArcSwap 实现无锁读取 |
| 配置收敛 | 资源级 skip / 硬错误保留旧快照；详见 `arch-data-model` |

## 为什么用 ArcSwap 而非 RwLock

- `RwLock` 在高并发下有读者饥饿风险
- `ArcSwap::load()` 无锁（原子指针读取）
- 仅 `store()` 需要短暂原子交换
