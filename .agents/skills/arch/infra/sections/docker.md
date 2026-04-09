# Docker Compose（开发环境）

```yaml
# aisix/docker-compose.yml
services:
  etcd:
    image: quay.io/coreos/etcd:v3.5
    ports: ["2379:2379"]
  redis:
    image: redis:7-alpine
    ports: ["6379:6379"]
```

网关二进制不在 docker-compose 中；直接通过 `cargo run` 运行。

## 冒烟测试

```bash
bash aisix/scripts/smoke-phase1.sh
```

需要运行中的网关 + etcd + Redis。
