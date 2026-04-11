# 网关启动配置（YAML）

文件：`aisix/config/aisix-gateway.example.yaml`

```yaml
server:
  listen: "0.0.0.0:4000"
  admin_listen: "127.0.0.1:4001"
  metrics_listen: "0.0.0.0:9090"
  request_body_limit_mb: 8
etcd:
  endpoints:
    - "http://127.0.0.1:2379"
  prefix: "/aisix"
  dial_timeout_ms: 5000
redis:
  url: "redis://127.0.0.1:6379"
log:
  level: "info"
runtime:
  worker_threads: 1
cache:
  default: "disabled"
deployment:
  admin:
    enabled: true
    admin_keys:
      - key: "change-me-admin-key"
```

使用示例配置时需设置 `OPENAI_API_KEY` 环境变量用于上游 Provider 认证。

约束：

- `server.admin_listen` 同时承载 Admin API 与 Admin UI
- `server.admin_listen` 不得与 `server.listen` 使用同一端口
- Admin UI 中的 admin key 由用户手动输入，只保存在浏览器 session 中

`cache.default` 提供网关进程级默认缓存策略；资源级覆盖不写入启动配置。
