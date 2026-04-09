# 健康检查

| 端点 | 用途 | 成功 | 失败 | 检查项 |
|------|------|------|------|--------|
| `GET /health` | 存活探针 | 200 | — | 进程存活 |
| `GET /ready` | 就绪探针 | 200 | 503 | 快照已加载 + Redis PING |

Kubernetes 探针：`/health` 用于 liveness，`/ready` 用于 readiness。
