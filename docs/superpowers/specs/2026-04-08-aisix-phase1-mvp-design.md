# AISIX Phase 1 设计文档

## 1. 背景

目标是在 `aisix-backend-architecture.md` 的约束内，为 `aisix` 的 Phase 1 形成可直接进入实现计划的设计基线。

本设计只基于以下前提：

- 仅阅读 `aisix-backend-architecture.md`
- 不读取 `litellm-data-plane-panorama-20260403.md`
- 不读取本地其他 git 仓库，尤其不参考 `git/apisix`
- 后续执行计划应支持逐步追加编写

## 2. Phase 1 范围

Phase 1 的目标不是构建完整 AI Gateway 平台，而是构建一个可运行、可验证、可演示的核心数据面 MVP。

Phase 1 必须包含：

- OpenAI 兼容 API：`/v1/chat/completions`、`/v1/embeddings`
- Provider 支持：OpenAI、Azure OpenAI、Anthropic
- 鉴权：仅 `Virtual Key`
- 路由：`model -> provider` 固定 1:1 映射
- 限流：Redis 支持 `RPM` / `TPM` / `并发`
- 缓存：仅非流式 chat completion 的进程内内存缓存
- 配置：etcd 全量加载、watch 热更新、断连重连后继续使用旧快照服务
- Spend：仅记录 input/output token usage，不做 budget enforcement
- 可观测：tracing、Prometheus、OTEL
- 响应头：`x-aisix-provider`、`x-aisix-cache-hit`
- 管理面：内嵌最小 Admin API
- 运维端点：`/health`、`/ready`
- 本地运行主路径：`docker-compose` 提供 `etcd + redis`，`aisix-gateway` 支持本地运行
- Demo 验证：至少一个真实 provider 可跑通，其余 provider 可通过 mock/integration 验证

## 3. 非目标

以下内容明确不纳入 Phase 1 执行计划：

- budget enforcement
- Team / Member 层级
- 多 deployment 路由
- fallback / cooldown / weighted / least-busy
- Guardrail callback 引擎
- Prompt Template / Request Mutation
- Redis 响应缓存
- embeddings 缓存
- Responses API / Images / Audio / Realtime / MCP
- 语义缓存
- KMS / Vault / 其他 secret backend 扩展
- 独立 `aisix-admin` 服务
- pricing / cost 计算 / `x-aisix-cost`
- callback sinks（Langfuse、Datadog）
- 企业级 graceful shutdown 语义

额外裁剪说明：

- 第十四章中的 `TC-13 spend 超限后请求被拒绝` 不纳入 Phase 1，本阶段只做 usage tracking，不做 budget reject

## 4. 实施策略

采用“纵向可运行切片”推进，而不是“先横向铺基础设施、最后再联调”。

推荐顺序：

1. Workspace + 非流式 Chat 基线
2. Embeddings 与 OpenAICompat 复用闭环
3. Redis 限流闭环
4. SSE 流式代理
5. 配置热加载与运行时稳定性
6. 内存缓存、Admin API、可观测、Demo 收尾

该顺序的目标：

- 每一轮结束后都更接近可运行 demo
- 将高风险项拆成独立切片
- 在严格按文档拆 crate 的前提下，仍保持任务足够小

## 5. Workspace 与 Crate 边界

Phase 1 严格按文档中的 workspace 结构落地，但每个 crate 只实现 MVP 所需最小职责。

保留的 crate：

- `bin/aisix-gateway`
- `aisix-types`
- `aisix-core`
- `aisix-config`
- `aisix-storage`
- `aisix-auth`
- `aisix-policy`
- `aisix-router`
- `aisix-ratelimit`
- `aisix-cache`
- `aisix-providers`
- `aisix-spend`
- `aisix-observability`
- `aisix-runtime`
- `aisix-server`

各 crate 的 Phase 1 边界：

### 5.1 `aisix-types`

负责共享类型：`Operation`、`CanonicalRequest`、`TransportMode`、`Usage`、`StreamEvent`、配置实体结构、对外错误体相关类型。

不负责：axum / reqwest / redis / etcd 依赖，不负责配置编译逻辑。

### 5.2 `aisix-core`

负责：`RequestContext`、`PostCallContext`、共享 trait 边界、`GatewayError`。

不负责：外部 IO、provider 实现、server 路由逻辑。

### 5.3 `aisix-config`

负责：etcd 原始实体解析、schema/semantic validation、`CompiledSnapshot`、watch 后重新编译。

不负责：HTTP handler、认证、限流判定、provider 调用。

### 5.4 `aisix-storage`

负责：Redis primitive、Lua/原子操作封装、并发租约与计数器基础 repo、启动配置中的 secret resolution。

不负责：限流策略判断、provider 编解码、HTTP 错误映射。

### 5.5 `aisix-auth`

负责：Bearer token 提取、Virtual Key 校验、认证入口。

不负责：allowed_models 判定、rate limit、provider 路由。

### 5.6 `aisix-policy`

负责：`allowed_models` 授权、operation 允许性校验、`policy_id` 与 inline `rate_limit` 合并、最小 `EffectivePolicy` 解析。

不负责：Redis 计数、provider 选择、Phase 2 的 mutation 能力。

### 5.7 `aisix-router`

负责：`model -> provider` 的 1:1 解析、route decision 结果、provider capability 基础匹配。

不负责：fallback、weighted、least-busy、cooldown。

### 5.8 `aisix-ratelimit`

负责：local shadow limiter、Redis authoritative precheck、request 完成后的 settle、concurrency guard 生命周期管理、Redis 故障降级策略。

不负责：policy 合并、usage 解析、HTTP response rendering。

### 5.9 `aisix-cache`

负责：memory cache backend、chat cache key builder、cache read/write、TTL 与容量控制。

不负责：Redis cache、embeddings 缓存、流式响应缓存。

### 5.10 `aisix-providers`

负责：`ProviderCodec`、`OpenAICompatCodec`、`AnthropicCodec`、provider request build / response parse、streaming normalization、provider error normalization。

不负责：业务限流、config watch、handler orchestration。

### 5.11 `aisix-spend`

负责：usage extraction 统一模型、usage event、post-call usage record。

不负责：pricing、budget enforcement、PostgreSQL。

### 5.12 `aisix-observability`

负责：tracing、metrics、OTEL 接线、结构化 usage/event logging。

不负责：业务判断。

### 5.13 `aisix-runtime`

负责：构建 `AppState`、初始化 clients / registries / caches / redis / snapshot loader、启动后台任务。

不负责：HTTP route handler、provider codec 细节、Admin 业务本体。

### 5.14 `aisix-server`

负责：axum router、chat / embeddings / admin / health / ready handler、request extraction、response rendering、`run_pipeline` 编排。

不负责：直接散布 etcd/redis/provider-specific 细节。

### 5.15 `bin/aisix-gateway`

负责：读取启动配置、初始化 runtime、启动 HTTP server。

## 6. 关键运行时闭环

### 6.1 请求数据流

Phase 1 采用固定阶段顺序，不做动态 pipeline 组装：

1. route match
2. decode request -> `CanonicalRequest`
3. init `RequestContext`
4. authenticate `Virtual Key`
5. authorize model access + resolve effective limits
6. rate limit precheck
7. cache lookup
8. route select
9. provider request build
10. upstream call
11. parse provider response
12. write response to client
13. async post-call usage record

Phase 1 请求链路显式裁剪：

- 不引入 Guardrail
- 不引入 Request Mutation
- 不引入 Fallback
- 不引入 Budget reject
- 不引入多 deployment route strategy

### 6.2 `RequestContext` 最小字段

`RequestContext` 在 Phase 1 仅保留这些跨阶段必要字段：

- `request_id`
- `operation`
- `request: CanonicalRequest`
- `key_meta`
- `effective_policy`
- `selected_target`
- `usage`
- `cache_hit`

### 6.3 配置数据流

配置进入运行时只允许一条标准路径：

1. etcd 保存原始实体
2. `aisix-config` 拉取并解析实体
3. 验证通过后编译成 `CompiledSnapshot`
4. `ArcSwap` 原子替换当前快照
5. 请求只读取快照，不直接读取 etcd

`CompiledSnapshot` 在 Phase 1 至少需要包含：

- `keys_by_token`
- `providers_by_id`
- `models_by_name`
- `policies_by_id`
- 供 runtime/provider 使用的已编译 provider 视图

### 6.4 热更新闭环

采用“debounce 后全量重编译快照”的 MVP 策略：

1. watch 收到 event
2. 进入 debounce 窗口
3. 基于最新实体集重新 compile
4. compile 成功才 `store(new_snapshot)`
5. compile 失败保留旧快照
6. watch 断开后后台重连

Phase 1 不做增量 patch snapshot。

### 6.5 Admin API 写路径

Admin API 只负责把资源写入 etcd，不直接改内存快照：

1. `POST/PUT/DELETE /admin/...`
2. server 校验 HTTP body 基本格式
3. 写入 etcd 指定前缀
4. 返回写入成功
5. 后台 watch 收到变更
6. recompile snapshot
7. 新配置对新请求生效

必须坚持：

- Admin API 不直接调用 runtime 内存更新逻辑

### 6.6 Admin API 最小资源范围

Phase 1 内嵌 Admin API 至少覆盖：

- `Provider`
- `Model`
- `ApiKey`

对 `Policy` 的设计结论：

- 读路径支持 `policy_id`
- 实现计划中建议将 `Policy CRUD` 作为 Admin 的后续子批次补齐
- getting started 主路径优先使用 inline `rate_limit`，避免演示路径过早依赖 policy 资源

## 7. 测试与验收策略

Phase 1 的测试目标是证明 MVP 关键闭环稳定，而不是覆盖未来全部能力。

### 7.1 单元测试重点

- `CanonicalRequest -> TransportMode`
- `allowed_models` 授权判定
- inline `rate_limit` 覆盖 `policy_id`
- cache key 构造稳定性
- provider error normalization
- OpenAI / Anthropic 流式 chunk 解析
- etcd entity -> `CompiledSnapshot` 编译验证
- `GatewayError -> OpenAI error response` 映射

### 7.2 Phase 1 必须自动化的集成测试

- 有效 API Key 通过
- 无效 API Key 返回 401
- 缺少 Authorization 返回 401
- inline RPM 限流触发
- `policy_id` 限流触发
- inline 覆盖 `policy_id`
- chat 非流式代理成功
- embeddings 代理成功
- chat 流式代理返回合法 SSE
- 流式结束后 usage 被记录
- model 路由到指定 provider
- 上游 5xx 归一化为 502
- 热更新 rate_limit 后立即生效
- etcd 启动不可用时 gateway 启动失败
- Redis 故障时降级为仅本地 shadow limiter

### 7.3 明确不纳入 Phase 1 自动化验收

- budget exceeded 拒绝
- 多 deployment fallback
- guardrail pre/during/post call
- Redis 响应缓存
- embeddings 缓存
- pricing / cost header
- audio / realtime / mcp

### 7.4 Provider 验证策略

- OpenAI compat：至少 1 条真实 E2E
- Azure OpenAI：mock/integration 为主
- Anthropic：mock/integration 为主

### 7.5 流式测试口径

流式测试至少断言：

- `content-type` 为 `text/event-stream`
- 至少包含一个 `data: {...}`
- 最终以 `data: [DONE]` 结束
- 非 `[DONE]` chunk 可解析为合法 OpenAI chunk
- 流结束后 usage settle 已落地
- 客户端中断时不会造成并发租约泄漏（至少需要明确验证方案）

### 7.6 Demo 手工验收

最终 demo 应支持以下手工演示流程：

1. 启动 `etcd` 和 `redis`
2. 启动 `aisix-gateway`
3. 通过 Admin API 创建 provider / model / apikey
4. 调用一次非流式 chat
5. 连续调用触发 rate limit
6. 重复相同非流式 chat，验证 `x-aisix-cache-hit: true`
7. 调用一次流式 chat，验证 SSE 与 `[DONE]`
8. 调用 embeddings
9. 更新 apikey 限流配置，验证热更新生效
10. 查看 `/health`、`/ready`、metrics 或 tracing 输出

## 8. Getting Started 交付要求

`getting started` 必须作为 Phase 1 交付物的一部分，而不是代码完成后的附带文档。

至少包含：

- `docker-compose.yml`：提供 `etcd + redis`
- 示例启动配置
- Admin API 初始化示例
- chat / embeddings curl 示例
- 流式请求示例
- 热更新示例
- 真实 provider 所需环境变量说明

推荐 demo 主路径：

1. `docker-compose up -d etcd redis`
2. 设置 `OPENAI_API_KEY`
3. `cargo run -p aisix-gateway`
4. 调用 Admin API 创建 provider / model / apikey
5. 调用 chat / embeddings
6. 修改配置验证热更新、限流和缓存响应头

## 9. 风险与执行约束

执行计划必须显式防止以下范围漂移与工程失控：

- 不把 Phase 2/3 能力提前混入 Phase 1
- 不为未来 plugin/pipeline 做过度抽象
- 不把 Admin API 写成直接改内存
- 不把流式 fallback 提前塞进 Phase 1
- 不把 budget reject 混入 spend tracking
- 不在 server handler 中散布 provider-specific 分支
- 不把测试主路径建立在真实云环境强依赖上

任务拆分必须遵守以下规则：

- 单任务尽量控制在半天到一天内可完成并验证
- 一个任务只交付一个清晰结果
- 单任务尽量只跨 1-3 个 crate
- 高风险点单独成任务
- 每个切片结束都必须形成更完整的“可运行状态”

## 10. 计划文档结构约束

后续实现计划文档建议采用以下结构：

1. 背景与范围
2. Phase 1 边界裁剪
3. workspace 与 crate 落地原则
4. 纵向实施切片总览
5. 详细任务列表
6. 测试与验收策略
7. demo 与 getting started 交付物
8. 风险清单与实现顺序约束

详细任务列表应按“纵向切片”为主组织；每个任务至少写清：

- 任务名称
- 目标结果
- 涉及 crate
- 前置依赖
- 完成定义
- 验证方式

为了适配长文档逐步追加，建议追加顺序为：

1. 范围与边界
2. 实施切片总览
3. 详细任务列表 Part 1
4. 详细任务列表 Part 2
5. 测试 / 验收 / demo
6. 风险与执行约束
