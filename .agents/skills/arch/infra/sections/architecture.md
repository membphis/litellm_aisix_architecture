# 架构：数据面 / 控制面硬分离

```
控制面 (aisix-admin)  ──写入──▶  etcd 集群  ──watch──▶  数据面 (aisix-gateway)
```

| 维度 | aisix-admin（控制面） | aisix-gateway（数据面） |
|------|----------------------|----------------------|
| 写入到 | etcd + PostgreSQL | 仅结构化日志 |
| 读取自 | PostgreSQL | etcd（watch）+ Redis |
| 处理 | Admin REST API | 所有 LLM 代理流量 |
| 运行方式 | 独立服务（Phase 4+） | Phase 1 内嵌 Admin API |

数据面节点从不直接调用控制面。通过 etcd 解耦。
