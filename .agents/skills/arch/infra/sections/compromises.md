# 当前阶段妥协

- [!NOTE] 尚无优雅关闭（Phase 3）。SIGTERM 会终止进行中的请求。
- [!NOTE] 仅单节点缓存。网关实例间不共享缓存。
- [!NOTE] Redis 和 etcd 连接无 TLS。安全边界依赖网络隔离。
- [!NOTE] Redis 无连接池。每次限流检查打开一个 TCP 连接。
  Phase 2 可能升级到 `redis` crate + `bb8` 连接池。
- [!NOTE] 网关二进制必须从仓库根目录使用 `--manifest-path aisix/Cargo.toml` 运行。
  所有 `cargo` 命令遵循此模式。
