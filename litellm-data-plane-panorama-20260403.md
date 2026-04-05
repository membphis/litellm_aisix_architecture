# LiteLLM 数据面功能全景图

> **定位**：LiteLLM = OpenAI 兼容的统一 LLM 网关（Proxy），同时也是 Python SDK。数据面指所有**运行时/请求路径上的能力**。
>
> 文档来源：https://docs.litellm.ai/docs/ | 版本时间：2026-04

---

## 📐 架构总览

```
Client (OpenAI SDK / curl)
  │
  ▼
LiteLLM Proxy (config.yaml)
  ├─ Auth Layer ──────── Virtual Key / JWT / Custom Auth
  ├─ Guardrail Layer ──── pre_call / during_call / post_call
  ├─ Router Layer ─────── Load Balance / Fallback / Retry / Tag Routing
  ├─ Cache Layer ──────── Redis / S3 / GCS / Qdrant Semantic
  ├─ LLM Call ─────────── 100+ Provider 统一适配
  ├─ Logging Layer ─────── Langfuse / Datadog / Prometheus / ...
  └─ Spend Tracking ───── Key / User / Team / Tag 维度计费
```

---

## 1️⃣ 模型管理（Model Management）

### 核心概念

| 概念 | 说明 |
|---|---|
| **model deployment** | 底层模型部署（如 `azure/gpt-4o-eu`），对应一个具体 endpoint + credential |
| **model group** | 用户可见的模型名（`model_name`），一个 group 下可挂多个 deployment 实现负载均衡 |
| **wildcard** | `model_name: "*"` 通配，自动路由到 provider 原生模型 |

### 经典配置

```yaml
model_list:
  # 单模型组 × 单部署
  - model_name: gpt-4o
    litellm_params:
      model: azure/gpt-4o-eu
      api_base: https://my-endpoint-europe.openai.azure.com/
      api_key: os.environ/AZURE_API_KEY_EU
      api_version: "2023-07-01-preview"
    model_info:
      mode: chat                    # chat | embedding | image_generation | audio_transcription
      input_cost_per_token: 0.00001 # 自定义计费
      output_cost_per_token: 0.00003
      max_tokens: 128000
      access_groups: ["beta-models"] # 模型访问组
      supported_environments: ["production", "staging"]
      custom_tokenizer:
        identifier: deepseek-ai/DeepSeek-V3-Base
        revision: main

  # 单模型组 × 多部署（自动负载均衡）
  - model_name: gpt-4o
    litellm_params:
      model: azure/gpt-4o-ca
      api_base: https://my-endpoint-canada.openai.azure.com/
      api_key: os.environ/AZURE_API_KEY_CA
      rpm: 100
      tpm: 100000

  # Wildcard — 代理 provider 所有模型
  - model_name: "*"
    litellm_params:
      model: "*"

  # Embedding 模型
  - model_name: text-embedding-ada-002
    litellm_params:
      model: azure/azure-embedding-model
      api_base: os.environ/AZURE_API_BASE
      api_key: os.environ/AZURE_API_KEY

  # 多 Organization 自动展开
  - model_name: "*"
    litellm_params:
      model: openai/*
      api_key: os.environ/OPENAI_API_KEY
      organization: [org-1, org-2, org-3]
```

### 集中凭据管理

```yaml
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: azure/gpt-4o
      litellm_credential_name: default_azure_credential  # 引用凭据

credential_list:
  - credential_name: default_azure_credential
    credential_values:
      api_key: os.environ/AZURE_API_KEY
      api_base: os.environ/AZURE_API_BASE
      api_version: "2023-05-15"
    credential_info:
      description: "Production credentials for EU region"
      custom_llm_provider: "azure"
```

### 自定义 Prompt 模板

```yaml
model_list:
  - model_name: mistral-7b
    litellm_params:
      model: "huggingface/mistralai/Mistral-7B-Instruct-v0.1"
      api_base: "<your-api-base>"
      initial_prompt_value: "\n"
      roles:
        system:   {pre_message: "<|im_start|>system\n",   post_message: "<|im_end|>"}
        assistant:{pre_message: "<|im_start|>assistant\n",post_message: "<|im_end|>"}
        user:     {pre_message: "<|im_start|>user\n",     post_message: "<|im_end|>"}
      final_prompt_value: "\n"
```

---

## 2️⃣ 路由与负载均衡（Routing & Load Balancing）

### 路由策略

| 策略 | 说明 | 适用场景 |
|---|---|---|
| `simple-shuffle` | 基于 tpm/rpm 权重的随机选择（**默认，推荐**） | 通用场景 |
| `least-busy` | 选当前最空闲的 deployment | 低延迟优先 |
| `latency-based-routing` | 基于历史延迟选择最快的 | 延迟敏感型 |
| `usage-based-routing` | 基于使用量均衡分配 | 成本均衡 |

### 经典配置

```yaml
router_settings:
  routing_strategy: simple-shuffle    # 推荐，配合 tpm/rpm 性能最优
  num_retries: 3                      # 每个模型组重试次数
  timeout: 30                         # 请求超时（秒）
  stream_timeout: 60                  # 流式请求超时
  model_group_alias:                  # 模型组别名
    gpt-4: gpt-4o
    claude-3: claude-3-haiku-20240229
  default_litellm_params:             # 全局默认参数
    temperature: 0.7
    max_tokens: 4096

  # 多实例部署（K8s 等）需配置 Redis 共享状态
  redis_host: os.environ/REDIS_HOST
  redis_password: os.environ/REDIS_PASSWORD
  redis_port: 1992

  # 健康检查
  enable_pre_call_checks: true        # 请求前检查 context window

  # Cooldown
  allowed_fails: 3                    # 1 分钟内失败 > 3 次则冷却
  cooldown_time: 30                   # 冷却时间（秒）

  # 精细化失败策略
  allowed_fails_policy:
    AuthenticationErrorAllowedFails: 10
    TimeoutErrorAllowedFails: 12
    RateLimitErrorAllowedFails: 10000
    InternalServerErrorAllowedFails: 20
```

### Fallbacks（降级链）

```yaml
litellm_settings:
  fallbacks: [{"gpt-4o": ["claude-3.5-sonnet", "gpt-3.5-turbo"]}]
  context_window_fallbacks: [{"gpt-3.5-turbo": ["gpt-3.5-turbo-16k"]}]
  content_policy_fallbacks: [{"gpt-3.5-turbo": ["claude-opus"]}]
  default_fallbacks: ["claude-opus"]   # 全局兜底
```

### Tag-Based Routing

```yaml
router_settings:
  enable_tag_filtering: true
  tag_filtering_match_any: true       # true=匹配任一 tag，false=匹配所有 tag

# 模型 deployment 上设置 tags
model_list:
  - model_name: gpt-4o
    litellm_params:
      model: azure/gpt-4o-eu
    model_info:
      tags: ["eu-region", "high-priority"]
```

---

## 3️⃣ API Key 与认证（Authentication）

### 认证方式

| 方式 | 说明 |
|---|---|
| **Virtual Key** | 自生成的 `sk-xxx` 格式 Key，支持 Budget/Rate Limit/Model Access |
| **JWT Auth** | OIDC/JWT Token 认证，`enable_jwt_auth: true` |
| **Custom Auth** | Python 自定义认证函数 |
| **IP Filtering** | `allowed_ips: ["1.2.3.4"]` |

### 经典配置

```yaml
general_settings:
  master_key: sk-1234                 # 管理员密钥（必须 sk- 开头）
  database_url: "postgresql://user:pass@host:5432/dbname"
  # custom_auth: custom_auth.my_auth_fn  # 自定义认证
  # enable_jwt_auth: true
  # allowed_ips: ["10.0.0.0/8"]
  litellm_key_header_name: "X-Litellm-Key"  # 自定义 Key Header
```

### Key 生命周期管理

```bash
# 生成 Key（带 Budget、Rate Limit、Model Access、过期时间）
curl 'http://0.0.0.0:4000/key/generate' \
  -H 'Authorization: Bearer sk-1234' \
  -H 'Content-Type: application/json' \
  -d '{
    "models": ["gpt-4o", "claude-3.5-sonnet"],
    "max_budget": 100,
    "budget_duration": "30d",
    "tpm_limit": 100000,
    "rpm_limit": 1000,
    "max_parallel_requests": 50,
    "duration": "90d",
    "aliases": {"gpt-3.5-turbo": "gpt-4o"},
    "metadata": {"team": "core-infra"},
    "team_id": "my-team-id",
    "user_id": "alice@company.com"
  }'

# Key 轮换（Enterprise）
curl 'http://0.0.0.0:4000/key/sk-xxx/regenerate' \
  -H 'Authorization: Bearer sk-1234' \
  -d '{"grace_period": "48h"}'

# 启用 / 禁用 Key
curl -X POST 'http://0.0.0.0:4000/key/block'   -d '{"key": "sk-xxx"}'
curl -X POST 'http://0.0.0.0:4000/key/unblock' -d '{"key": "sk-xxx"}'
```

### Key 生成约束

```yaml
litellm_settings:
  # 生成 Key 时的上限约束
  upperbound_key_generate_params:
    max_budget: 100
    budget_duration: "10d"
    duration: "30d"
    max_parallel_requests: 1000
    tpm_limit: 100000
    rpm_limit: 1000

  # 生成 Key 时的默认值
  default_key_generate_params:
    max_budget: 1.5
    models: ["gpt-3.5-turbo"]
    metadata: {"setting": "default"}

  # 限制谁能生成 Key
  key_generation_settings:
    team_key_generation:
      allowed_team_member_roles: ["admin"]
      required_params: ["tags"]
    personal_key_generation:
      allowed_user_roles: ["proxy_admin"]
```

---

## 4️⃣ 预算与限流（Budgets & Rate Limits）

### 限流维度

| 维度 | 参数 | 说明 |
|---|---|---|
| **TPM** | `tpm_limit` | Tokens Per Minute |
| **RPM** | `rpm_limit` | Requests Per Minute |
| **Max Parallel** | `max_parallel_requests` | 最大并发请求数 |
| **Max Budget** | `max_budget` | 美元预算上限 |
| **Budget Duration** | `budget_duration` | 预算重置周期（30s/30m/30h/30d） |
| **Per-Model RPM/TPM** | `model_rpm_limit` / `model_tpm_limit` | 按 model_name 设置不同限流 |
| **Per-Model Budget** | `model_max_budget` | 按 model_name 设置不同预算 |
| **TPM 计数方式** | `token_rate_limit_type` | `total`(默认) / `input` / `output` |

### 作用层级（从粗到细）

```
Global Proxy Budget
  └─ Team Budget (+ per-model RPM/TPM)
       └─ Team Member Budget (max_budget_in_team)
            └─ Virtual Key Budget (+ per-model budget)
                 └─ Customer (end-user) Budget
```

### 经典配置

```yaml
# 全局限流
general_settings:
  max_parallel_requests: 100
  global_max_parallel_requests: 1000
  token_rate_limit_type: "output"     # TPM 只计算 output tokens

litellm_settings:
  max_budget: 1000                    # 全局预算
  budget_duration: "30d"              # 30 天重置
  max_end_user_budget: 0.0001         # 终端用户预算
  max_internal_user_budget: 10        # 内部用户默认预算
  internal_user_budget_duration: "1mo"
```

```bash
# Key 级别限流
/key/generate  →  tpm_limit, rpm_limit, max_budget, budget_duration, model_rpm_limit, model_tpm_limit, model_max_budget

# Team 级别限流
/team/new      →  tpm_limit, rpm_limit, max_budget, budget_duration, model_rpm_limit, model_tpm_limit

# Agent 级别限流
/v1/agents     →  tpm_limit, rpm_limit, session_tpm_limit, session_rpm_limit, max_iterations, max_budget_per_session
```

---

## 5️⃣ 缓存（Caching）

### 支持的缓存类型

| 类型 | `cache_params.type` | 说明 |
|---|---|---|
| Redis | `redis` | **默认**，生产推荐 |
| In-Memory | `local` | 测试用 |
| Disk | `disk` | 本地磁盘 |
| Qdrant Semantic | `qdrant-semantic` | 语义相似度缓存 |
| Redis Semantic | `redis-semantic` | Redis + 语义搜索 |
| S3 | `s3` | AWS S3 |
| GCS | `gcs` | Google Cloud Storage |

### 经典配置

```yaml
litellm_settings:
  cache: true
  cache_params:
    type: redis
    host: os.environ/REDIS_HOST
    port: 6379
    password: os.environ/REDIS_PASSWORD
    namespace: "litellm.caching"
    ttl: 600                              # 默认 TTL（秒）
    max_connections: 100
    supported_call_types:                 # 启用缓存的调用类型
      - acompletion                       # /chat/completions
      - aembedding                        # /embeddings
      - atranscription                    # /audio/transcriptions
    # mode: default_off                   # 默认关闭，按需开启

    # Qdrant 语义缓存
    # type: qdrant-semantic
    # qdrant_semantic_cache_embedding_model: openai-embedding
    # qdrant_collection_name: litellm_cache
    # similarity_threshold: 0.8

    # S3 缓存
    # type: s3
    # s3_bucket_name: cache-bucket-litellm
    # s3_region_name: us-west-2
```

### 请求级缓存控制

```json
{"cache": {"ttl": 300}}          // 缓存 5 分钟
{"cache": {"s-maxage": 600}}     // 只用 10 分钟内的缓存
{"cache": {"no-cache": true}}    // 跳过缓存
{"cache": {"no-store": true}}    // 不存储到缓存
{"cache": {"namespace": "my-ns"}} // 自定义命名空间
```

---

## 6️⃣ Guardrails（安全护栏）

### 执行时机

| Mode | 时机 | 作用对象 |
|---|---|---|
| `pre_call` | LLM 调用前 | 输入 |
| `during_call` | 与 LLM 调用并行 | 输入 |
| `post_call` | LLM 调用后 | 输入 + 输出 |
| `logging_only` | 仅日志记录 | 不拦截 |

### 支持的 Guardrail Provider

| Provider | `guardrail` 值 |
|---|---|
| Aporia | `aporia` |
| Lakera | `lakera` |
| Aim | `aim` |
| AWS Bedrock | `bedrock` |
| Guardrails AI | `guardrails_ai` |
| Presidio (PII) | `presidio` |
| Azure Text Moderation | `azure/text_moderations` |
| Hide Secrets | `hide-secrets` |
| 通用 HTTP API | `generic_guardrail_api` |

### 经典配置

```yaml
guardrails:
  - guardrail_name: "pii-masker"
    litellm_params:
      guardrail: presidio
      mode: "pre_call"
      default_on: true              # 所有请求默认执行
      presidio_language: "en"
      pii_entities_config:
        CREDIT_CARD: "MASK"
        EMAIL_ADDRESS: "MASK"
        US_SSN: "MASK"
      presidio_score_thresholds:
        CREDIT_CARD: 0.8
        EMAIL_ADDRESS: 0.6

  - guardrail_name: "content-safety"
    litellm_params:
      guardrail: aporia
      mode: ["pre_call", "post_call"]  # 支持组合 mode
      api_key: os.environ/APORIA_API_KEY
      api_base: os.environ/APORIA_API_BASE

  - guardrail_name: "custom-guard"
    litellm_params:
      guardrail: generic_guardrail_api
      mode: [pre_call, post_call]
      api_base: https://api.security.example.com/v1/litellm
      api_key: os.environ/GUARD_API_KEY
```

### 请求级使用

```json
// 显式指定 guardrails
{"guardrails": ["pii-masker", "content-safety"]}

// 传动态参数（Enterprise）
{"guardrails": {"pii-masker": {"extra_body": {"threshold": 0.9}}}}
```

---

## 7️⃣ 可观测性与日志（Observability & Logging）

### 回调类型

| 设置 | 说明 |
|---|---|
| `success_callback` | 成功时触发 |
| `failure_callback` | 失败时触发 |
| `callbacks` | 成功和失败都触发 |
| `service_callbacks` | 系统健康监控（Redis/Postgres 故障） |

### 经典配置

```yaml
litellm_settings:
  success_callback: ["langfuse"]
  failure_callback: ["sentry"]
  callbacks: ["otel"]                     # OpenTelemetry
  service_callbacks: ["datadog", "prometheus"]

  # 日志隐私控制
  turn_off_message_logging: true          # 不记录消息内容（仅 metadata）
  redact_user_api_key_info: true          # 脱敏 API Key 信息

  # Langfuse 特定
  langfuse_default_tags: ["cache_hit", "user_api_key_alias"]
```

---

## 8️⃣ 费用追踪（Spend Tracking）

### 计费维度

```
Key Spend   → /key/info?key=sk-xxx
User Spend  → /user/info?user_id=alice
Team Spend  → /team/info?team_id=team-1
Tag Spend   → metadata.tags（Enterprise）
End User    → request body 的 "user" 字段
```

### 请求级追踪

```json
{
  "model": "gpt-4o",
  "messages": [...],
  "user": "customer-123",
  "metadata": {
    "tags": ["job:21459", "task:classification"],
    "spend_logs_metadata": {"project": "alpha"}
  }
}
```

### 响应头

```
x-litellm-response-cost: 0.0001065         // 本次请求费用
x-litellm-cache-key: 586bf3f3c1bf...       // 缓存 Key
x-litellm-applied-guardrails: pii-masker   // 已应用的 guardrails
x-litellm-key-remaining-requests-gpt-4o: 99
x-litellm-key-remaining-tokens-gpt-4o: 99900
```

### Spend Report（Enterprise）

```bash
# 按 Team 分组
/global/spend/report?start_date=2024-04-01&end_date=2024-06-30&group_by=team

# 按 Customer 分组
/global/spend/report?group_by=customer

# 按 API Key
/global/spend/report?api_key=sk-xxx

# 日活跃度
/user/daily/activity?start_date=2025-03-20&end_date=2025-03-27

# 单条日志
/spend/logs?start_date=2024-01-01&summarize=false
```

---

## 9️⃣ Retry 与超时（Retry & Timeout）

```yaml
litellm_settings:
  request_timeout: 600                  # 全局请求超时（秒），默认 6000
  num_retries: 3                        # 重试次数

router_settings:
  timeout: 30                           # Router 级超时
  stream_timeout: 60                    # 流式超时
  num_retries: 2

  # 按错误类型精细化重试
  retry_policy:
    AuthenticationErrorRetries: 3
    TimeoutErrorRetries: 3
    RateLimitErrorRetries: 3
    ContentPolicyViolationErrorRetries: 4
    InternalServerErrorRetries: 4
```

---

## 🔟 多模态支持（Multi-Modal）

| 能力 | Endpoint | 说明 |
|---|---|---|
| Chat Completion | `/chat/completions` | 文本、Vision（图片输入） |
| Embeddings | `/embeddings` | 文本向量化 |
| Image Generation | `/images/generations` | DALL-E、Stable Diffusion 等 |
| Audio Transcription | `/audio/transcriptions` | Whisper 等 |
| Audio Speech | `/audio/speech` | TTS |
| Realtime API | WebSocket | 实时语音对话 |
| Responses API | `/v1/responses` | OpenAI Responses API |

---

## 1️⃣1️⃣ MCP & A2A Gateway

```yaml
# MCP Aliases
litellm_settings:
  mcp_aliases:
    github: "github_mcp_server"
    zapier: "zapier_mcp_server"

# MCP 管理
general_settings:
  user_mcp_management_mode: "restricted"  # restricted | view_all
  enable_mcp_registry: true
```

---

## 1️⃣2️⃣ 密钥管理（Secret Management）

| 方式 | 设置 |
|---|---|
| 环境变量引用 | `os.environ/ENV_VAR_NAME` |
| AWS KMS | `key_management_system: aws_kms` |
| Google KMS | `key_management_system: google_kms` |
| Azure Key Vault | `use_azure_key_vault: true` |
| AWS Secret Manager | Enterprise |
| HashiCorp Vault | Enterprise |

---

## 1️⃣3️⃣ 健康检查与告警

```yaml
general_settings:
  background_health_checks: true
  health_check_interval: 300            # 秒
  alerting: ["slack", "email"]
  alerting_threshold: 300               # 触发告警的阈值
  alerting_args:
    slack_webhook_url: os.environ/SLACK_WEBHOOK_URL

router_settings:
  alerting_config:
    slack_alerting: true
    slack_web_hook_url: os.environ/SLACK_WEBHOOK_URL
```

---

## 1️⃣4️⃣ 其他数据面设置

| 设置 | 位置 | 说明 |
|---|---|---|
| `drop_params` | `litellm_settings` | 自动丢弃 provider 不支持的参数 |
| `modify_params` | `litellm_settings` | 允许在发送前修改参数 |
| `force_ipv4` | `litellm_settings` | 强制 IPv4（解决 Anthropic 连接问题） |
| `json_logs` | `litellm_settings` | JSON 格式日志 |
| `forward_client_headers_to_llm_api` | `general_settings` | 转发客户端 Header |
| `max_request_size_mb` | `general_settings` | 请求体大小上限 |
| `max_response_size_mb` | `general_settings` | 响应体大小上限 |
| `always_include_stream_usage` | `general_settings` | 流式响应始终包含 usage |
| `enforce_user_param` | `general_settings` | 强制要求 user 参数 |
| `completion_model` | `general_settings` | 全局覆盖 model 参数 |
| `store_model_in_db` | `general_settings` | 模型配置存数据库 |
| `default_team_disabled` | `general_settings` | 禁止创建无 Team 的 Key |

---

## 📋 全景速查表

| 功能域 | 关键配置段 | 核心参数 |
|---|---|---|
| 模型管理 | `model_list` | `model_name`, `litellm_params.model`, `model_info` |
| 路由 | `router_settings` | `routing_strategy`, `fallbacks`, `num_retries`, `timeout` |
| 认证 | `general_settings` | `master_key`, `database_url`, `custom_auth` |
| 预算限流 | `/key/generate`, `/team/new` | `max_budget`, `budget_duration`, `tpm_limit`, `rpm_limit` |
| 缓存 | `litellm_settings.cache` | `cache_params.type`, `cache_params.ttl` |
| Guardrails | `guardrails` | `guardrail`, `mode`, `default_on` |
| 日志 | `litellm_settings` | `success_callback`, `failure_callback` |
| 计费 | 请求 `metadata` | `tags`, `spend_logs_metadata`, `user` |
| 重试 | `router_settings` | `retry_policy`, `allowed_fails_policy` |
| 多模态 | `model_list` | `mode: embedding\|image_generation\|audio_transcription` |
| 密钥 | `general_settings` | `key_management_system`, `os.environ/` |
| 健康检查 | `general_settings` | `background_health_checks`, `health_check_interval` |
| MCP/A2A | `litellm_settings` | `mcp_aliases` |
| Agent | `/v1/agents` | `session_tpm_limit`, `max_iterations`, `max_budget_per_session` |

---

## 🚀 生产级全功能配置示例

```yaml
# ===== litellm_config.yaml — 生产级全功能配置 =====

model_list:
  - model_name: gpt-4o
    litellm_params:
      model: azure/gpt-4o
      api_base: os.environ/AZURE_API_BASE
      api_key: os.environ/AZURE_API_KEY
      api_version: "2024-02-01"
      rpm: 200
      tpm: 200000
    model_info:
      access_groups: ["production-models"]
      supported_environments: ["production"]

  - model_name: gpt-4o
    litellm_params:
      model: openai/gpt-4o
      api_key: os.environ/OPENAI_API_KEY
      rpm: 100
    model_info:
      access_groups: ["production-models"]

  - model_name: claude-sonnet
    litellm_params:
      model: anthropic/claude-sonnet-4-20250514
      api_key: os.environ/ANTHROPIC_API_KEY

  - model_name: text-embedding
    litellm_params:
      model: openai/text-embedding-3-small
      api_key: os.environ/OPENAI_API_KEY

litellm_settings:
  drop_params: true
  request_timeout: 60
  num_retries: 3
  cache: true
  cache_params:
    type: redis
    host: os.environ/REDIS_HOST
    port: 6379
    password: os.environ/REDIS_PASSWORD
    ttl: 600
  success_callback: ["langfuse"]
  failure_callback: ["sentry"]
  fallbacks: [{"gpt-4o": ["claude-sonnet"]}]

router_settings:
  routing_strategy: simple-shuffle
  redis_host: os.environ/REDIS_HOST
  redis_password: os.environ/REDIS_PASSWORD
  redis_port: 6379
  timeout: 30
  stream_timeout: 60
  allowed_fails: 3
  cooldown_time: 30

guardrails:
  - guardrail_name: "pii-protection"
    litellm_params:
      guardrail: presidio
      mode: "pre_call"
      default_on: true
      pii_entities_config:
        CREDIT_CARD: "MASK"
        EMAIL_ADDRESS: "MASK"

general_settings:
  master_key: os.environ/LITELLM_MASTER_KEY
  database_url: os.environ/DATABASE_URL
  database_connection_pool_limit: 10
  background_health_checks: true
  health_check_interval: 300
  alerting: ["slack"]
  max_request_size_mb: 10

environment_variables:
  REDIS_HOST: os.environ/REDIS_HOST
  REDIS_PASSWORD: os.environ/REDIS_PASSWORD
```

---

## 核心要点总结

1. **模型层**：`model_name`(group) → `litellm_params.model`(deployment)，天然支持负载均衡
2. **路由层**：4 种策略 + fallback + tag routing + cooldown，Router 是整个数据面的调度中枢
3. **认证层**：Virtual Key 为核心，支持 JWT/Custom Auth/IP Filter
4. **限流层**：5 层粒度（Global → Team → Team Member → Key → Customer），支持 per-model RPM/TPM/Budget
5. **缓存层**：7 种后端，支持语义缓存，请求级动态控制
6. **Guardrail 层**：3 个执行时机 × 9+ Provider，支持 per-key/per-model/per-tag 精细控制
7. **可观测层**：Callback 机制接 Langfuse/Datadog/Prometheus 等，Spend 按多维度聚合
