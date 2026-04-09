# 网关启动配置（YAML）

文件：`aisix/config/aisix-gateway.example.yaml`

```yaml
server:
  listen: "0.0.0.0:8080"
etcd:
  endpoints: ["http://127.0.0.1:2379"]
  prefix: "/aisix"
redis:
  url: "redis://127.0.0.1:6379"
log:
  level: "info"
admin:
  key: "admin-secret-key"
```

使用示例配置时需设置 `OPENAI_API_KEY` 环境变量用于上游 Provider 认证。
