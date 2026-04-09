# Admin Dependency Validation Alignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让 AISIX 项目与所有 `arch-*` project skill 对齐到新的契约：Admin API 在写入前拒绝依赖无效的资源，而不是继续把依赖补齐职责交给 watcher 侧收敛。

**Architecture:** 保留 `compile_snapshot()` 与 watcher 当前的容错行为，继续把它们作为运行时防御性过滤机制；同时把正常控制面写入路径的引用完整性校验前移到 Admin API 边界。新的成功语义是：Admin `PUT` 只有在路径/body id 校验通过、依赖校验通过、并且 etcd 接受写入后才返回成功。运行时编译仍然会跳过那些通过直接写 etcd、删除级联或历史脏数据产生的失效资源。

**Tech Stack:** Rust、Axum、Tokio、etcd、workspace crates（`aisix-server`、`aisix-types`、`aisix-config`）、位于 `.agents/skills/arch/` 下的 Markdown project skill

---

## File Map

### Existing files to modify

- `.agents/skills/arch/api-design/SKILL.md`
  - 把当前“允许乱序写入”的说明替换为新的 Admin 前置依赖校验契约。
- `.agents/skills/arch/data-model/SKILL.md`
  - 把 compile 阶段的 skip 语义重新定位为运行时防御性过滤，而不是公开的 Admin API 写入契约。
- `.agents/skills/arch/infra/SKILL.md`
  - 更新 watcher / 热加载相关表述，明确 watcher 不再承担正常依赖收敛职责。
- `.agents/skills/arch/style/SKILL.md`
  - 补一条边界规则：正常控制面校验应发生在 Admin 边界，而不是依赖 watcher 侧 skip 逻辑。
- `aisix/crates/aisix-types/src/error.rs`
  - 新增依赖冲突错误类型，并映射到 HTTP `409 Conflict`。
- `aisix/crates/aisix-server/src/admin/mod.rs`
  - 增加可复用的引用校验辅助函数，以及写入前依赖检查逻辑。
- `aisix/crates/aisix-server/src/admin/providers.rs`
  - 保持 handler 薄封装，但 provider 写入需要经过 policy 引用校验。
- `aisix/crates/aisix-server/src/admin/models.rs`
  - 保持 handler 薄封装，但 model 写入需要经过 provider/policy 校验。
- `aisix/crates/aisix-server/src/admin/apikeys.rs`
  - 保持 handler 薄封装，但 apikey 写入需要经过 model/policy 校验。
- `aisix/crates/aisix-server/tests/admin_reload.rs`
  - 把原来“依赖无效写入仍 200 OK”的覆盖改成拒绝测试，并保留有序写入成功路径。
- `aisix/docs/admin-api.md`
  - 更新 Admin API 语义、示例和错误码说明。
- `aisix/README.md`
  - 删除“依赖无效写入成功后由 watcher 继续收敛”的对外表述。
- `docs/architecture.md`
  - 更新控制面与热加载架构描述，明确 Admin 校验和 watcher 防御过滤的分工。

### Existing files to reuse without planned modification

- `aisix/crates/aisix-config/src/compile.rs`
  - 保持现有 `CompileIssue` 机制，继续作为运行时防御性过滤。
- `aisix/crates/aisix-config/src/watcher.rs`
  - 保持当前“发布有效子集”的行为，仅通过文档与测试重新界定其职责。
- `aisix/crates/aisix-config/tests/snapshot_compile.rs`
  - 现有 compile 测试继续作为运行时防御覆盖。
- `aisix/crates/aisix-config/tests/etcd_watch.rs`
  - 现有 watcher 测试继续作为异常 etcd 状态容忍覆盖。

### Test commands to use during execution

- `cargo test --manifest-path aisix/Cargo.toml -p aisix-types error -- --nocapture`
- `cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_reload -- --nocapture`
- `cargo test --manifest-path aisix/Cargo.toml -p aisix-config snapshot_compile -- --nocapture`
- `cargo test --manifest-path aisix/Cargo.toml -p aisix-config etcd_watch -- --nocapture`
- `cargo test --manifest-path aisix/Cargo.toml`
- `cargo clippy --manifest-path aisix/Cargo.toml -- -D warnings`

## Task 1: 更新 `arch-api-design`，切换到新的 Admin 契约

**Files:**
- Modify: `.agents/skills/arch/api-design/SKILL.md`

- [ ] **Step 1: 重写 skill 中的 Admin 写入语义段落**

保留你这次在 `api-design` 里新增的核心方向，也就是“改为不允许乱序写入，依赖关系必须在 Admin 写入阶段满足”，但允许重新润色措辞，不要求保留原句。

把 `.agents/skills/arch/api-design/SKILL.md` 中当前的写入语义段替换为下面这段：

```md
### 写入语义

Admin `PUT` 在写入 **etcd** 前先做引用校验。
成功的 `PUT`/`DELETE` 意味着：

1. 请求通过了路径/body id 校验
2. 请求通过了依赖校验
3. etcd 接受了写入

成功响应仍不代表运行时快照已更新；watcher 会异步 reload 并发布新快照。

依赖校验规则：

- `provider.policy_id` 非空时，该 policy 必须已存在
- `model.provider_id` 必须已存在
- `model.policy_id` 非空时，该 policy 必须已存在
- `apikey.allowed_models` 中所有 model 必须已存在
- `apikey.policy_id` 非空时，该 policy 必须已存在

watcher/`compile_snapshot()` 仍保留缺依赖资源 skip 能力，但那是对直接写 etcd、删除级联和历史脏数据的防御性兜底，不是正常 Admin API 契约的一部分。
```

- [ ] **Step 2: 删除旧的“设计内容”与历史表述，只保留新的正式契约**

在 `.agents/skills/arch/api-design/SKILL.md` 中检查写入语义附近是否还残留下面这类旧内容；如果还在，全部删除，而不是保留为注释、引用块或历史说明：

```md
> 允许乱序写入（例如先写 model 再写其 provider）。收敛在 watcher 层完成；依赖补齐后，后续 reload 会自动纳入该资源。
修改为不允许乱序写入（必须先写 provider），会增加客户端复杂度（必须处理依赖关系），但简化服务器端实现（不需要 watcher 处理依赖补齐）。当前阶段选择允许乱序写入以降低集成门槛。
```

删除目标是：`arch-api-design` 最终只保留新的正式设计结论，不保留“之前如何设计”或“为什么从旧设计改成新设计”的过程性描述。

- [ ] **Step 3: 更新 skill 中的 Admin 错误码表**

把 Admin 错误码表替换为：

```md
### 错误码

| HTTP | 条件 |
|------|------|
| 401 | 缺失/无效 admin key |
| 400 | 无效 id 或路径/body 不匹配 |
| 409 | 依赖未满足（缺失 provider / model / policy） |
| 404 | 资源不存在（GET/DELETE） |
| 500 | etcd 或服务器故障 |
```

- [ ] **Step 4: 验证 skill 文档已明确表达新方向，且旧设计内容已清除**

Run: `rg -n "允许乱序写入|修改为不允许乱序写入|降低集成门槛|409|依赖校验|防御性兜底" .agents/skills/arch/api-design/SKILL.md`
Expected: `409`、`依赖校验`、`防御性兜底` 能匹配到；`允许乱序写入`、`修改为不允许乱序写入`、`降低集成门槛` 都不再出现。

## Task 2: 同步更新其余 `arch-*` skills

**Files:**
- Modify: `.agents/skills/arch/data-model/SKILL.md`
- Modify: `.agents/skills/arch/infra/SKILL.md`
- Modify: `.agents/skills/arch/style/SKILL.md`

- [ ] **Step 1: 在 `arch-data-model` 中把 compile 语义改写为防御性过滤**

把 `.agents/skills/arch/data-model/SKILL.md` 中“配置编译规则”尾部替换成下面这段：

```md
资源级语义：

- provider 引用缺失 policy → 该 provider 跳过
- model 引用缺失 provider 或 policy → 该 model 跳过
- apikey 引用缺失 model 或 policy → 该 apikey 跳过
- 被跳过资源在当前运行时快照中视为 absent，不保留旧版本

发布语义：

- 仅存在 `CompileIssue` 时，watcher 发布有效资源子集并记录日志
- 出现硬错误（如 duplicate id / duplicate token）时，本次发布失败，旧快照继续生效

边界说明：

- 正常 Admin API 写入路径应在写 etcd 前完成依赖校验
- `CompileIssue` skip 语义用于处理直接写 etcd、删除级联、历史脏数据或 watch 中间态
- 因此 `compile_snapshot()` 的 skip 能力是运行时防御机制，不定义控制面成功写入契约
```

- [ ] **Step 2: 更新 `arch-infra`，明确 watcher 不再承担正常依赖收敛**

把 `.agents/skills/arch/infra/SKILL.md` 热加载表格中的这一行改成：

```md
| 配置收敛 | watcher 负责 reload + compile + 原子发布；缺依赖 skip 属于防御性过滤，正常依赖校验由 Admin API 在写入前完成 |
```

同时把启动序列后的说明改成：

```md
5. `compile_snapshot()` 产出编译报告并发布可用快照
6. `ArcSwap.store()` — 网关开始服务
7. 从 revision N+1 启动后台 watcher

正常控制面写入应在 Admin API 边界完成引用校验；watcher 仍容忍异常 etcd 状态并过滤掉依赖失效资源。
```

- [ ] **Step 3: 在 `arch-style` 中增加一条边界规则**

在 `.agents/skills/arch/style/SKILL.md` 的错误处理或 pipeline 约定附近追加这条规则：

```md
- 能在 Admin API 边界前置拒绝的引用错误，不应主要依赖 watcher/compile 的 skip 逻辑作为正常控制流
```

- [ ] **Step 4: 验证所有 skill 已指向同一方向**

Run: `rg -n "乱序写入|依赖校验|防御性|skip 语义|409" .agents/skills/arch`
Expected: 新增的 `依赖校验` / `防御性` / `409` 能在目标 skill 中找到，`乱序写入` 不再作为当前契约出现。

## Task 3: 新增依赖冲突错误类型

**Files:**
- Modify: `aisix/crates/aisix-types/src/error.rs`
- Test: `aisix/crates/aisix-types/src/error.rs`

- [ ] **Step 1: 先写失败测试，定义新的错误映射**

在 `aisix/crates/aisix-types/src/error.rs` 末尾追加下面两个测试：

```rust
#[test]
fn maps_conflict_to_invalid_request_error_type() {
    assert_eq!(error_type(StatusCode::CONFLICT), "invalid_request_error");
}

#[test]
fn dependency_conflict_uses_http_conflict() {
    let error = GatewayError {
        kind: ErrorKind::Conflict,
        message: "model 'gpt-4o-mini' references missing provider 'openai'".to_string(),
    };

    assert_eq!(error.status_code(), StatusCode::CONFLICT);
}
```

- [ ] **Step 2: 运行目标测试，确认当前实现尚未支持**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-types maps_conflict_to_invalid_request_error_type -- --nocapture`
Expected: FAIL，因为当前 `ErrorKind::Conflict` 还不存在。

- [ ] **Step 3: 在 `error.rs` 中增加 `Conflict` 并更新状态码映射**

把 `aisix/crates/aisix-types/src/error.rs` 的关键部分改成下面这样：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Authentication,
    Permission,
    NotFound,
    InvalidRequest,
    Conflict,
    RateLimited,
    Timeout,
    Upstream,
    Internal,
}

impl GatewayError {
    pub fn status_code(&self) -> StatusCode {
        match self.kind {
            ErrorKind::Authentication => StatusCode::UNAUTHORIZED,
            ErrorKind::Permission => StatusCode::FORBIDDEN,
            ErrorKind::NotFound => StatusCode::NOT_FOUND,
            ErrorKind::InvalidRequest => StatusCode::BAD_REQUEST,
            ErrorKind::Conflict => StatusCode::CONFLICT,
            ErrorKind::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            ErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
            ErrorKind::Upstream => StatusCode::BAD_GATEWAY,
            ErrorKind::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

fn error_type(status: StatusCode) -> &'static str {
    match status {
        StatusCode::UNAUTHORIZED => "authentication_error",
        StatusCode::FORBIDDEN => "permission_error",
        StatusCode::NOT_FOUND => "invalid_request_error",
        StatusCode::BAD_REQUEST => "invalid_request_error",
        StatusCode::CONFLICT => "invalid_request_error",
        StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
        _ => "server_error",
    }
}
```

- [ ] **Step 4: 重新运行 `aisix-types` 目标测试**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-types error -- --nocapture`
Expected: PASS，新的 `409 Conflict` 映射被覆盖。

## Task 4: 在 Admin 写入路径增加前置引用校验

**Files:**
- Modify: `aisix/crates/aisix-server/src/admin/mod.rs`
- Modify: `aisix/crates/aisix-server/src/admin/providers.rs`
- Modify: `aisix/crates/aisix-server/src/admin/models.rs`
- Modify: `aisix/crates/aisix-server/src/admin/apikeys.rs`

- [ ] **Step 1: 先写失败测试，定义 model 缺 provider 时必须被拒绝**

在 `aisix/crates/aisix-server/tests/admin_reload.rs` 中追加下面这个测试：

```rust
#[tokio::test]
async fn admin_put_model_rejects_missing_provider_before_etcd_write() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let response = app
        .clone()
        .oneshot(admin_put_request(
            "/admin/models/gpt-4o-mini",
            json!(ModelConfig {
                id: "gpt-4o-mini".to_string(),
                provider_id: "missing-provider".to_string(),
                upstream_model: "gpt-4o-mini".to_string(),
                policy_id: None,
                rate_limit: None,
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["error"]["message"],
        "model 'gpt-4o-mini' references missing provider 'missing-provider'"
    );
    assert_eq!(json["error"]["type"], "invalid_request_error");
}
```

- [ ] **Step 2: 运行目标测试，确认当前行为仍是错误的**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_put_model_rejects_missing_provider_before_etcd_write -- --nocapture`
Expected: FAIL，因为当前 Admin API 仍然接受该写入并返回 `200 OK`。

- [ ] **Step 3: 在 `admin/mod.rs` 中增加可复用的依赖校验辅助函数**

把 `aisix/crates/aisix-server/src/admin/mod.rs` 的 `AdminState` 实现改成下面这种形状，在现有 `put/get/list/delete` 私有辅助函数之前插入这些方法：

```rust
impl AdminState {
    pub async fn put_provider(
        &self,
        id: &str,
        provider: ProviderConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.ensure_policy_reference(provider.policy_id.as_deref(), "provider", id)
            .await?;
        self.put("providers", id, &provider).await
    }

    pub async fn put_model(
        &self,
        id: &str,
        model: ModelConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.ensure_existing_provider(&model.provider_id, &model.id).await?;
        self.ensure_policy_reference(model.policy_id.as_deref(), "model", id)
            .await?;
        self.put("models", id, &model).await
    }

    pub async fn put_apikey(
        &self,
        id: &str,
        apikey: ApiKeyConfig,
    ) -> Result<AdminWriteResult, GatewayError> {
        self.ensure_existing_models(&apikey.allowed_models, &apikey.id)
            .await?;
        self.ensure_policy_reference(apikey.policy_id.as_deref(), "api key", id)
            .await?;
        self.put("apikeys", id, &apikey).await
    }

    async fn ensure_existing_provider(
        &self,
        provider_id: &str,
        model_id: &str,
    ) -> Result<(), GatewayError> {
        let mut etcd = self.etcd.lock().await;
        let provider = etcd
            .get_json::<ProviderConfig>(&self.prefix, "providers", provider_id)
            .await
            .map_err(internal_admin_error)?;

        if provider.is_some() {
            return Ok(());
        }

        Err(GatewayError {
            kind: ErrorKind::Conflict,
            message: format!(
                "model '{model_id}' references missing provider '{provider_id}'"
            ),
        })
    }

    async fn ensure_existing_models(
        &self,
        model_ids: &[String],
        key_id: &str,
    ) -> Result<(), GatewayError> {
        let mut etcd = self.etcd.lock().await;
        for model_id in model_ids {
            let model = etcd
                .get_json::<ModelConfig>(&self.prefix, "models", model_id)
                .await
                .map_err(internal_admin_error)?;
            if model.is_none() {
                return Err(GatewayError {
                    kind: ErrorKind::Conflict,
                    message: format!(
                        "api key '{key_id}' references missing model '{model_id}'"
                    ),
                });
            }
        }
        Ok(())
    }

    async fn ensure_policy_reference(
        &self,
        policy_id: Option<&str>,
        resource_kind: &str,
        resource_id: &str,
    ) -> Result<(), GatewayError> {
        let Some(policy_id) = policy_id else {
            return Ok(());
        };

        let mut etcd = self.etcd.lock().await;
        let policy = etcd
            .get_json::<PolicyConfig>(&self.prefix, "policies", policy_id)
            .await
            .map_err(internal_admin_error)?;

        if policy.is_some() {
            return Ok(());
        }

        Err(GatewayError {
            kind: ErrorKind::Conflict,
            message: format!(
                "{resource_kind} '{resource_id}' references missing policy '{policy_id}'"
            ),
        })
    }
}
```

- [ ] **Step 4: 保持各个 handler 继续是薄封装**

`providers.rs`、`models.rs`、`apikeys.rs` 不要扩展成大块业务逻辑，改完后仍应保持这种形状：

```rust
pub async fn put_model(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(model): Json<ModelConfig>,
) -> Result<Json<crate::admin::AdminWriteResult>, GatewayError> {
    let admin = require_admin(&state, &headers)?;
    ensure_path_matches_body_id(&id, &model.id)?;
    let result = admin.put_model(&id, model).await?;
    Ok(Json(result))
}
```

- [ ] **Step 5: 重新运行目标 Admin 校验测试**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_put_model_rejects_missing_provider_before_etcd_write -- --nocapture`
Expected: PASS，返回 `409 Conflict`，错误消息与测试一致。

## Task 5: 更新 Admin 集成测试，锁定新的行为契约

**Files:**
- Modify: `aisix/crates/aisix-server/tests/admin_reload.rs`

- [ ] **Step 1: 替换原有“无效依赖写入仍成功”的断言**

在 `aisix/crates/aisix-server/tests/admin_reload.rs` 中，把下面这段断言：

```rust
        let invalid_update = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/models/gpt-4o-mini",
                json!(ModelConfig {
                    id: "gpt-4o-mini".to_string(),
                    provider_id: "missing-provider".to_string(),
                    upstream_model: "gpt-4o-mini-2024-07-18".to_string(),
                    policy_id: None,
                    rate_limit: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(invalid_update.status(), StatusCode::OK);
```

替换为：

```rust
        let invalid_update = app
            .clone()
            .oneshot(admin_put_request(
                "/admin/models/gpt-4o-mini",
                json!(ModelConfig {
                    id: "gpt-4o-mini".to_string(),
                    provider_id: "missing-provider".to_string(),
                    upstream_model: "gpt-4o-mini-2024-07-18".to_string(),
                    policy_id: None,
                    rate_limit: None,
                }),
            ))
            .await
            .unwrap();
        assert_eq!(invalid_update.status(), StatusCode::CONFLICT);
```

- [ ] **Step 2: 增加 provider / apikey 两类依赖失败测试**

在 `aisix/crates/aisix-server/tests/admin_reload.rs` 中追加下面两个测试：

```rust
#[tokio::test]
async fn admin_put_provider_rejects_missing_policy_before_etcd_write() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let response = app
        .clone()
        .oneshot(admin_put_request(
            "/admin/providers/openai",
            json!(ProviderConfig {
                id: "openai".to_string(),
                kind: ProviderKind::OpenAi,
                base_url: "https://api.openai.com".to_string(),
                auth: ProviderAuth {
                    secret_ref: "env:OPENAI_API_KEY".to_string(),
                },
                policy_id: Some("missing-policy".to_string()),
                rate_limit: None,
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn admin_put_apikey_rejects_missing_models_before_etcd_write() {
    let fixture = LiveEtcdTestApp::start().await;
    let app = fixture.router();

    let response = app
        .clone()
        .oneshot(admin_put_request(
            "/admin/apikeys/demo-key",
            json!(ApiKeyConfig {
                id: "demo-key".to_string(),
                key: "sk-demo-phase1".to_string(),
                allowed_models: vec!["missing-model".to_string()],
                policy_id: None,
                rate_limit: None,
            }),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CONFLICT);
}
```

- [ ] **Step 3: 保留一个“有序写入成功”的 happy path 测试**

确保 `admin_can_create_provider_model_and_apikey_then_gateway_uses_reloaded_snapshot()` 继续作为标准成功路径，并保持顺序：

```rust
provider -> model -> apikey -> proxy request
```

- [ ] **Step 4: 运行 Admin 集成测试**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_reload -- --nocapture`
Expected: PASS；有序写入成功，依赖无效写入返回 `409`。

## Task 6: 更新项目文档，与新契约对齐

**Files:**
- Modify: `aisix/docs/admin-api.md`
- Modify: `aisix/README.md`
- Modify: `docs/architecture.md`

- [ ] **Step 1: 更新 `aisix/docs/admin-api.md` 的概述与错误码**

把开头的语义说明替换成下面这段：

```md
The Admin API manages runtime config stored in etcd under the configured prefix.

Admin `PUT` requests perform referential validation before writing to etcd.
For example, a `model` must reference an existing `provider`, and an `apikey` must only reference existing `model` ids.

Admin writes are accepted by etcd first and applied to the live gateway later by the background watcher.
This means a successful `PUT` or `DELETE` response confirms the config change passed validation and was stored in etcd, not that the new runtime snapshot is already active.

`compile_snapshot()` and the watcher still skip dependency-invalid resources seen in etcd, but that behavior is a defensive runtime safeguard for direct etcd edits or delete cascades, not part of the normal Admin API write contract.
```

同时把错误码列表替换成：

```md
- `401 Unauthorized`: missing or invalid `x-admin-key`
- `400 Bad Request`: invalid resource id or path/body id mismatch
- `409 Conflict`: missing referenced provider, model, or policy
- `404 Not Found`: missing resource on item `GET` or `DELETE`
- `500 Internal Server Error`: etcd or server-side failure
```

- [ ] **Step 2: 用新的失败示例替换旧的乱序写入示例**

把 `Put a Model Before Its Provider Exists` 这一节替换成：

```md
### Put a Model With a Missing Provider

This is rejected before the write reaches etcd:

```bash
curl -fsS -X PUT http://127.0.0.1:4000/admin/models/gpt-4o-mini \
  -H 'content-type: application/json' \
  -H 'x-admin-key: change-me-admin-key' \
  -d '{
    "id": "gpt-4o-mini",
    "provider_id": "openai",
    "upstream_model": "gpt-4o-mini",
    "policy_id": null,
    "rate_limit": null
  }'
```

If `openai` does not already exist under `/admin/providers/openai`, the API returns `409 Conflict` and no write is stored.
```

- [ ] **Step 3: 更新 `aisix/README.md` 中 Admin 语义段落**

把 `aisix/README.md` 中第 27-29 行附近的说明替换为：

```md
The embedded Admin API writes config into etcd under the configured prefix. Admin `PUT` requests validate cross-resource references before writing, so dependency-invalid writes fail fast with `409 Conflict`. A successful Admin response means etcd accepted a validated write; it does not guarantee the new config is already active.

The watcher still recompiles runtime snapshots asynchronously. If etcd later contains invalid resource graphs because of direct edits, delete cascades, or historical data, runtime compilation skips those resources defensively while preserving valid ones.
```

- [ ] **Step 4: 更新 `docs/architecture.md` 中 watch 路径与失败保护说明**

把 watch 路径中的这段：

```md
  ├── on PUT event
  │    ├── debounce (250-500ms merge window)
  │    ├── merge changes into local config
  │    ├── re-validate + recompile
  │    └── ArcSwap atomic swap (keep old snapshot if validation fails)
```

替换为：

```md
  ├── on PUT event
  │    ├── debounce (250-500ms merge window)
  │    ├── reload current etcd state
  │    ├── recompile valid runtime subset
  │    └── ArcSwap atomic swap (keep old snapshot only on hard compile failure)
```

并把失败保护表格行替换为：

```md
| **失败保护** | Admin API 前置拒绝依赖错误；watcher 对异常 etcd 状态执行防御性过滤，只有硬错误才保留旧快照 |
```

- [ ] **Step 5: 验证文档已不再承诺“乱序写入自动收敛”**

Run: `rg -n "concurrent writes across related resources|allow out-of-order|乱序写入|409 Conflict|防御性" aisix/docs/admin-api.md aisix/README.md docs/architecture.md`
Expected: `409 Conflict` 和防御性表述存在；不再把乱序写入收敛当作正常 Admin 契约。

## Task 7: 最终验证

**Files:**
- Modify: none
- Test: workspace verification only

- [ ] **Step 1: 运行聚焦测试**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-types error -- --nocapture && cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_reload -- --nocapture && cargo test --manifest-path aisix/Cargo.toml -p aisix-config snapshot_compile -- --nocapture && cargo test --manifest-path aisix/Cargo.toml -p aisix-config etcd_watch -- --nocapture`
Expected: 所有目标测试通过。

- [ ] **Step 2: 运行全量 workspace 测试**

Run: `cargo test --manifest-path aisix/Cargo.toml`
Expected: 全量测试通过。

- [ ] **Step 3: 运行 clippy**

Run: `cargo clippy --manifest-path aisix/Cargo.toml -- -D warnings`
Expected: 通过且无 warning。

- [ ] **Step 4: 用一条 diff 检查整套契约是否一致**

Run: `git diff -- .agents/skills/arch/api-design/SKILL.md .agents/skills/arch/data-model/SKILL.md .agents/skills/arch/infra/SKILL.md .agents/skills/arch/style/SKILL.md aisix/crates/aisix-types/src/error.rs aisix/crates/aisix-server/src/admin/mod.rs aisix/crates/aisix-server/src/admin/providers.rs aisix/crates/aisix-server/src/admin/models.rs aisix/crates/aisix-server/src/admin/apikeys.rs aisix/crates/aisix-server/tests/admin_reload.rs aisix/docs/admin-api.md aisix/README.md docs/architecture.md`
Expected: diff 中呈现的是同一套统一契约：Admin 用 `409` 拒绝依赖无效写入，watcher 继续保留防御性过滤能力，所有 `arch-*` skill 与项目文档都与该方向一致。
