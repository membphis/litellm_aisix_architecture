# 当前阶段妥协

- [!NOTE] API Key 以明文存储在 etcd 中。安全边界依赖 etcd 访问控制 + TLS。
  Phase 3 将迁移到 BLAKE3/SHA256 哈希存储，并集成 KMS/Vault。
- [!NOTE] Secret 解析仅支持 `env:` 前缀（`env:OPENAI_API_KEY`）。
  Phase 3 新增 AWS KMS、Vault、Azure Key Vault 后端。
- [!NOTE] Phase 1 缓存仅限进程内内存（moka/DashMap，LRU，上限 10000 条，TTL 300s）。
  Phase 2 新增 Redis 共享缓存；Phase 3 新增语义缓存（Qdrant）。
