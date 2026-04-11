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

按需加载分段，避免全文注入上下文。

## 分段目录

| 分段 | 说明 | 文件 |
|------|------|------|
| 架构分离 | 数据面 listener / admin listener 分离、角色对比 | `sections/architecture.md` |
| 外部依赖 | etcd（必需）、Redis（限流）、启动顺序 | `sections/dependencies.md` |
| 启动配置 | YAML 配置文件格式与 admin listener 约束 | `sections/config.md` |
| 热加载 | etcd watch → 防抖 → 编译 → ArcSwap、安全保障 | `sections/hot-reload.md` |
| 启动序列 | 7 步启动流程 | `sections/startup.md` |
| Crate 选型 | 依赖列表、最小化哲学 | `sections/crate-selection.md` |
| 可观测性 | Prometheus 指标、tracing、结构化日志 | `sections/observability.md` |
| 健康检查 | /health、/ready、Kubernetes 探针 | `sections/health.md` |
| Docker Compose | 开发环境配置 | `sections/docker.md` |
| 阶段路线图 | Phase 1-4 规划 | `sections/roadmap.md` |
| 当前妥协 | Phase 1 限制 | `sections/compromises.md` |

## 使用方式

根据当前任务需要的主题，用 Read 工具加载对应的 `sections/*.md` 文件，而非全文。
