# 网关启动配置（YAML）

文件：`aisix/config/aisix-gateway.example.yaml`

```yaml
server:
  listen: "0.0.0.0:4000"
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

`cache.default` 提供网关进程级默认缓存策略；资源级覆盖不写入启动配置。
