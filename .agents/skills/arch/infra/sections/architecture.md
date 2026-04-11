# 架构：数据面 listener / admin listener 分离

```
Admin listener (aisix-gateway)  ──写入──▶  etcd 集群  ──watch──▶  Data plane listener (aisix-gateway)
```

| 维度 | Admin listener（控制面） | Data plane listener（数据面） |
|------|---------------------------|--------------------------------|
| 写入到 | etcd | 仅结构化日志 |
| 读取自 | etcd（admin write path） | etcd（watch）+ Redis |
| 处理 | Admin REST API + `/ui` | 所有 LLM 代理流量 |
| 运行方式 | 与 `aisix-gateway` 同进程，不同 listener | 与 admin listener 同进程，不同 listener |

约束：`server.admin_listen` 与 `server.listen` 不能重叠。Admin API 与 Admin UI 共用 admin listener，数据面 listener 不暴露 `/admin/...` 或 `/ui`。
