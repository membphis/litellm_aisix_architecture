# 外部依赖

## etcd（必需）

- **角色**：所有运行时配置的唯一真相来源
- **连接**：标准 etcd 客户端，通过 `etcd-client` crate
- **Key 前缀**：可配置，默认 `/aisix`
- **启动行为**：etcd 不可达时网关**拒绝启动**（fail-fast）；若 etcd 可达但部分资源依赖无效，网关加载有效子集并启动
- **运行时行为**：watch 断开后自动重连；重连期间旧快照继续服务

## Redis（限流必需）

- **角色**：分布式限流计数器、并发租约、冷却标记
- **连接**：基于原始 `tokio::net::TcpStream` 的自定义最小 RESP 客户端
- **故障降级**：Redis 宕机时降级为本地影子限流器（内存 GCRA）。
  请求仍然被处理（可用性优先）；精度降低但基本保护仍在。
- **Phase 1 限制**：无连接池、无 TLS、无 Pub/Sub
- **使用命令**：`INCR`、`INCRBY`、`EXPIRE`、Sorted Set 操作

## 依赖启动顺序

```bash
docker compose -f aisix/docker-compose.yml up -d redis etcd
```

两者必须在网关启动前运行。etcd 不可达时网关立即失败。
