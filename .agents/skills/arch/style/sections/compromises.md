# 当前阶段妥协

- [!NOTE] 没有 plugin/pipeline 系统。预计 1-2 个季度后根据生产经验重新评估。
- [!NOTE] 自定义最小化 Redis 客户端（原始 RESP 协议）。无连接池、无 TLS。
  这避免了重量级依赖，但功能受限。Phase 2 可能升级到 `redis` crate。
- [!NOTE] 公开项尚未添加 `///` 文档注释。外部文档仅存在于 `docs/` 目录。
