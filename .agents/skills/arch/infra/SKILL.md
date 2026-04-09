---
name: arch-infra
description: AISIX 基础设施、部署、外部依赖和运维约束
trigger:
  files:
    - "aisix/docker-compose.yml"
    - "aisix/config/**"
    - "aisix/crates/aisix-storage/**"
    - "aisix/crates/aisix-observability/**"
    - "aisix/crates/aisix-runtime/**"
    - "aisix/bin/**"
    - "aisix/scripts/**"
    - "aisix/Cargo.toml"
  keywords:
    - "etcd"
    - "redis"
    - "docker"
    - "deployment"
    - "infrastructure"
    - "bootstrap"
    - "health check"
    - "graceful shutdown"
    - "observability"
    - "metrics"
    - "tracing"
    - "config yaml"
    - "startup"
    - "watcher"
    - "arc swap"
priority: high
related:
  - arch-style
  - arch-data-model
  - arch-api-design
---

# AISIX 基础设施与部署

## 架构：数据面 / 控制面硬分离

```
控制面 (aisix-admin)  ──写入──▶  etcd 集群  ──watch──▶  数据面 (aisix-gateway)
```

| 维度 | aisix-admin（控制面） | aisix-gateway（数据面） |
|------|----------------------|----------------------|
| 写入到 | etcd + PostgreSQL | 仅结构化日志 |
| 读取自 | PostgreSQL | etcd（watch）+ Redis |
| 处理 | Admin REST API | 所有 LLM 代理流量 |
| 运行方式 | 独立服务（Phase 4+） | Phase 1 内嵌 Admin API |

数据面节点从不直接调用控制面。通过 etcd 解耦。

## 外部依赖

### etcd（必需）

- **角色**：所有运行时配置的唯一真相来源
- **连接**：标准 etcd 客户端，通过 `etcd-client` crate
- **Key 前缀**：可配置，默认 `/aisix`
- **启动行为**：etcd 不可达时网关**拒绝启动**（fail-fast）；若 etcd 可达但部分资源依赖无效，网关加载有效子集并启动
- **运行时行为**：watch 断开后自动重连；重连期间旧快照继续服务

### Redis（限流必需）

- **角色**：分布式限流计数器、并发租约、冷却标记
- **连接**：基于原始 `tokio::net::TcpStream` 的自定义最小 RESP 客户端
- **故障降级**：Redis 宕机时降级为本地影子限流器（内存 GCRA）。
  请求仍然被处理（可用性优先）；精度降低但基本保护仍在。
- **Phase 1 限制**：无连接池、无 TLS、无 Pub/Sub
- **使用命令**：`INCR`、`INCRBY`、`EXPIRE`、Sorted Set 操作

### 依赖启动顺序

```bash
docker compose -f aisix/docker-compose.yml up -d redis etcd
```

两者必须在网关启动前运行。etcd 不可达时网关立即失败。

## 网关启动配置（YAML）

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

## 热加载机制

### 流程

```
etcd watch 事件 (revision N+1)
  → 防抖窗口（250-500ms）
  → 后台 tokio 任务：重新编译快照
  → ArcSwap.store(new_snapshot)（< 1μs，无锁）
  → 新请求自动使用新快照
  → 进行中的请求持有旧快照（Arc 引用计数）
```

### 安全保障

| 机制 | 说明 |
|------|------|
| 防抖窗口 | 250-500ms 合并窗口，处理快速连续变更 |
| 后台编译 | 不阻塞请求路径 |
| 原子指针交换 | 通过 ArcSwap 实现无锁读取 |
| 配置收敛 | 资源级 skip / 硬错误保留旧快照；详见 `arch-data-model` |

### 为什么用 ArcSwap 而非 RwLock

- `RwLock` 在高并发下有读者饥饿风险
- `ArcSwap::load()` 无锁（原子指针读取）
- 仅 `store()` 需要短暂原子交换

## 启动序列

1. 解析 YAML 启动配置
2. 连接 etcd，GET 完整前缀范围（捕获 revision N）
3. 解析 secret 引用（`env:KEY` → 实际值）
4. 执行三层校验（schema → 语义 → 运行时预检）
5. `compile_snapshot()` 产出编译报告并发布可用快照
6. `ArcSwap.store()` — 网关开始服务
7. 从 revision N+1 启动后台 watcher

配置编译的详细规则（哪些资源会被跳过、哪些错误会阻止发布）统一以 `arch-data-model` 为准。

## Crate 选型

| 用途 | Crate | 备注 |
|------|-------|------|
| HTTP 服务 | `axum` + `tower` | Tower 中间件处理横切关注点 |
| HTTP 客户端 | `reqwest` | 上游 Provider 调用，连接池 |
| 异步运行时 | `tokio` | 多线程调度器 |
| 序列化 | `serde` + `serde_json` + `serde_yaml` | |
| etcd 客户端 | `etcd-client` | |
| Redis | 自定义 RESP 客户端 | Phase 1 最小化；Phase 2 可能升级 |
| 限流 | `governor` | GCRA 算法用于影子限流器 |
| 指标 | `prometheus` | |
| 链路追踪 | `tracing` + `tracing-subscriber` | |
| 原子配置 | `arc-swap` | 无锁快照交换 |
| 时间戳 | `chrono` | |
| 字节 | `bytes` | 热路径零拷贝 |
| UUID | `uuid` | 请求 ID |
| 错误派生 | `thiserror` | 仅基础设施错误 |

### 最小化依赖哲学

12 个共享 workspace 依赖。不使用 `redis` crate（自定义客户端）。
领域代码中不使用 `anyhow`。无插件框架。

## 可观测性

### 指标（Prometheus）

在指标端口暴露（推荐 9090）。核心指标：
- 按 model/provider 分类的请求延迟直方图
- Token 用量计数器（输入/输出/缓存）
- 限流命中/未命中计数器
- 缓存命中率
- 上游错误率

### 链路追踪

`tracing` crate + 结构化 span。通过 `tower_http::trace::TraceLayer`
对所有 HTTP 请求自动埋点。

### 结构化日志

每个请求输出 `UsageEvent`，structlog 格式：
- Request ID、Key ID、Model、Provider
- 输入/输出 Token、费用
- 延迟、缓存命中状态
- Phase 2+：callback sink（Langfuse、Datadog）

## 健康检查

| 端点 | 用途 | 成功 | 失败 | 检查项 |
|------|------|------|------|--------|
| `GET /health` | 存活探针 | 200 | — | 进程存活 |
| `GET /ready` | 就绪探针 | 200 | 503 | 快照已加载 + Redis PING |

Kubernetes 探针：`/health` 用于 liveness，`/ready` 用于 readiness。

## Docker Compose（开发环境）

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

## 阶段路线图

### Phase 1 — MVP（4-6 周，当前阶段）

- `/v1/chat/completions` + `/v1/embeddings`
- OpenAI、Azure、Anthropic Provider
- Virtual Key 认证、基础限流、内存缓存
- 内嵌 Admin API、健康检查端点
- etcd 配置 + 热加载

### Phase 2 — 生产基线（4-6 周）

- 多 deployment 路由、fallback 策略
- 预算层级、按 model 限流
- Provider 健康检查 + 冷却
- Redis 共享缓存、Prompt 模板
- Callback sink（Langfuse、Datadog）

### Phase 3 — 企业级（4-8 周）

- Guardrail HTTP 回调引擎
- Secret 后端（KMS、Vault）
- 语义缓存（Qdrant）
- 更多 Provider 适配器
- 优雅关闭

### Phase 4 — 高级功能（8+ 周）

- Realtime WebSocket API、MCP 网关
- 独立控制面服务
- 多区域金丝雀部署

## 当前阶段妥协

- [!NOTE] 尚无优雅关闭（Phase 3）。SIGTERM 会终止进行中的请求。
- [!NOTE] 仅单节点缓存。网关实例间不共享缓存。
- [!NOTE] Redis 和 etcd 连接无 TLS。安全边界依赖网络隔离。
- [!NOTE] Redis 无连接池。每次限流检查打开一个 TCP 连接。
  Phase 2 可能升级到 `redis` crate + `bb8` 连接池。
- [!NOTE] 网关二进制必须从仓库根目录使用 `--manifest-path aisix/Cargo.toml` 运行。
  所有 `cargo` 命令遵循此模式。
