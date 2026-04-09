# 阶段路线图

## Phase 1 — MVP（4-6 周，当前阶段）

- `/v1/chat/completions` + `/v1/embeddings`
- OpenAI、Azure、Anthropic Provider
- Virtual Key 认证、基础限流、内存缓存
- 内嵌 Admin API、健康检查端点
- etcd 配置 + 热加载

## Phase 2 — 生产基线（4-6 周）

- 多 deployment 路由、fallback 策略
- 预算层级、按 model 限流
- Provider 健康检查 + 冷却
- Redis 共享缓存、Prompt 模板
- Callback sink（Langfuse、Datadog）

## Phase 3 — 企业级（4-8 周）

- Guardrail HTTP 回调引擎
- Secret 后端（KMS、Vault）
- 语义缓存（Qdrant）
- 更多 Provider 适配器
- 优雅关闭

## Phase 4 — 高级功能（8+ 周）

- Realtime WebSocket API、MCP 网关
- 独立控制面服务
- 多区域金丝雀部署
