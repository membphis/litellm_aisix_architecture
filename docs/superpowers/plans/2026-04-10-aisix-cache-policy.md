# AISIX 缓存策略实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 AISIX 增加可配置的全局默认缓存开关，以及 `provider` / `model` 级三态缓存策略，并按 `model > provider > global default` 在运行时决定是否读写缓存。

**Architecture:** 全局默认值来自启动 YAML，不进入 etcd 编译快照；`provider` / `model` 的 `cache.mode` 进入 `CompiledSnapshot` 作为资源级静态策略；运行时只在非流式 chat JSON 请求上解析最终缓存开关并控制缓存读写与响应头行为。

**Tech Stack:** Rust, serde, serde_yaml, axum, ArcSwap, Cargo tests

---

## 文件结构

- 修改：`aisix/crates/aisix-config/src/startup.rs`
  责任：启动 YAML 配置 schema，新增全局默认缓存开关。
- 修改：`aisix/config/aisix-gateway.example.yaml`
  责任：示例启动配置，展示 `cache.default`。
- 修改：`aisix/crates/aisix-core/src/app_state.rs`
  责任：保存运行时全局默认缓存布尔值。
- 修改：`aisix/crates/aisix-runtime/src/bootstrap.rs`
  责任：从启动配置解析全局默认缓存开关并注入 `AppState`。
- 修改：`aisix/crates/aisix-config/src/etcd_model.rs`
  责任：为 `provider`、`model` 增加三态缓存策略 schema。
- 修改：`aisix/crates/aisix-config/src/snapshot.rs`
  责任：把资源级缓存模式编译进快照。
- 修改：`aisix/crates/aisix-config/src/compile.rs`
  责任：编译 provider/model 的缓存模式映射。
- 修改：`aisix/crates/aisix-server/src/pipeline/cache.rs`
  责任：实现统一缓存开关判定函数，收口缓存读写逻辑。
- 修改：`aisix/crates/aisix-server/src/handlers/chat.rs`
  责任：读缓存前按最终策略判断是否启用缓存。
- 修改：`aisix/crates/aisix-server/src/pipeline/stream_chunk.rs`
  责任：写缓存前按最终策略判断是否启用缓存，并控制 `x-aisix-cache-hit` 头。
- 修改：`aisix/crates/aisix-config/tests/startup_config.rs`
  责任：验证启动配置默认值和示例配置。
- 修改：`aisix/crates/aisix-config/tests/snapshot_compile.rs`
  责任：验证三态缓存模式编译。
- 修改：`aisix/crates/aisix-runtime/tests/bootstrap.rs`
  责任：验证 bootstrap 接线后的结构构造。
- 修改：`aisix/crates/aisix-server/tests/chat_non_stream.rs`
  责任：验证运行时缓存优先级与命中行为。
- 修改：`aisix/crates/aisix-config/tests/etcd_loader.rs`
  责任：更新 JSON fixture，覆盖新字段兼容性。
- 修改：`aisix/crates/aisix-config/tests/etcd_watch.rs`
  责任：更新快照构造与 watcher 场景。
- 修改：`aisix/crates/aisix-server/tests/rate_limit.rs`
  责任：更新 `StartupConfig` / `AppState` helper。
- 修改：`aisix/crates/aisix-server/tests/admin_reload.rs`
  责任：更新 `StartupConfig` helper 与 provider/model JSON fixture。
- 修改：`aisix/README.md`
  责任：更新使用说明和缓存策略优先级。
- 修改：`aisix/docs/admin-api.md`
  责任：更新 provider/model 资源示例。
- 修改：`.agents/skills/arch/infra/sections/config.md`
  责任：把启动配置中的全局默认缓存开关反向同步到架构 skill。
- 修改：`.agents/skills/arch/data-model/sections/entities.md`
  责任：把 provider/model 的缓存 schema 反向同步到架构 skill。
- 修改：`.agents/skills/arch/data-model/sections/compilation.md`
  责任：把缓存模式编译与运行时解析规则反向同步到架构 skill。

## 最终规则

- 全局默认缓存开关暴露在启动 YAML 配置文件中。
- `provider` 和 `model` 支持三态缓存模式：`inherit | enabled | disabled`。
- 不配置 `cache` 字段时，等价于 `inherit`。
- 运行时最终决策顺序：`model > provider > global default`。
- 默认全局策略为 `disabled`。
- 第一版不支持 `key` 级静态缓存开关。
- 第一版不把缓存配置挂在 `policy` 上。
- 仅非流式 `chat` JSON 请求参与该缓存策略判定。
- 缓存关闭时，不查缓存、不写缓存，也不返回 `x-aisix-cache-hit` 响应头。
- 缓存开启但未命中时，返回 `x-aisix-cache-hit: false`。
- 缓存命中时，返回 `x-aisix-cache-hit: true`。
- 方案落地后，必须把对应架构事实反向同步到 `.agents` 中的架构 skill，保证 agent 使用的架构知识始终与代码和正式文档一致。

## 数据结构约定

### 启动配置

启动 YAML 新增：

```yaml
cache:
  default: "disabled"
```

Rust 建议新增：

```rust
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CacheConfig {
    #[serde(default)]
    pub default: CacheDefaultMode,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            default: CacheDefaultMode::Disabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
pub enum CacheDefaultMode {
    #[serde(rename = "enabled")]
    Enabled,
    #[default]
    #[serde(rename = "disabled")]
    Disabled,
}
```

### 资源配置

`provider` / `model` 增加：

```json
"cache": { "mode": "inherit" }
```

Rust 建议新增：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CacheMode {
    #[serde(rename = "inherit")]
    Inherit,
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachePolicyConfig {
    pub mode: CacheMode,
}
```

### 编译快照

在 `CompiledSnapshot` 中新增：

```rust
pub provider_cache_modes: HashMap<String, CacheMode>,
pub model_cache_modes: HashMap<String, CacheMode>,
```

说明：

- 快照只保存资源级缓存模式。
- 全局默认值不进入快照，因为它来自启动配置而不是 etcd。

### AppState

`AppState` 增加：

```rust
pub default_cache_enabled: bool,
```

## 任务拆解

### Task 1: 启动配置与运行时默认值接线

**Files:**
- Modify: `aisix/crates/aisix-config/src/startup.rs`
- Modify: `aisix/config/aisix-gateway.example.yaml`
- Modify: `aisix/crates/aisix-core/src/app_state.rs`
- Modify: `aisix/crates/aisix-runtime/src/bootstrap.rs`
- Test: `aisix/crates/aisix-config/tests/startup_config.rs`
- Test: `aisix/crates/aisix-runtime/tests/bootstrap.rs`

- [ ] **Step 1: 写失败测试，锁定启动配置默认值行为**

在 `aisix/crates/aisix-config/tests/startup_config.rs` 添加或调整测试：

```rust
use aisix_config::startup::{load_from_path, CacheDefaultMode};
use std::fs;

#[test]
fn loads_example_startup_config() {
    let path = format!(
        "{}/../../config/aisix-gateway.example.yaml",
        env!("CARGO_MANIFEST_DIR")
    );
    let config = load_from_path(&path).expect("example config should load");

    assert_eq!(config.server.listen, "0.0.0.0:4000");
    assert_eq!(config.etcd.prefix, "/aisix");
    assert_eq!(config.cache.default, CacheDefaultMode::Disabled);
    assert!(config
        .deployment
        .admin
        .admin_keys
        .first()
        .is_some_and(|key| !key.key.is_empty()));
}

#[test]
fn missing_cache_section_defaults_to_disabled() {
    let temp_dir = std::env::temp_dir();
    let path = temp_dir.join(format!("aisix-startup-config-{}.yaml", std::process::id()));

    fs::write(
        &path,
        r#"server:
  listen: "0.0.0.0:4000"
  metrics_listen: "0.0.0.0:9090"
  request_body_limit_mb: 8
etcd:
  endpoints:
    - "http://127.0.0.1:2379"
  prefix: "/aisix"
  dial_timeout_ms: 5000
redis:
  url: "redis://127.0.0.1:6379"
log:
  level: "info"
runtime:
  worker_threads: 0
deployment:
  admin:
    enabled: false
"#,
    )
    .expect("temporary config should be written");

    let config = load_from_path(path.to_str().expect("temp path should be valid utf-8"))
        .expect("config without cache section should load");

    assert_eq!(config.cache.default, CacheDefaultMode::Disabled);

    fs::remove_file(path).expect("temporary config should be cleaned up");
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config startup_config -- --nocapture`
Expected: FAIL，提示 `StartupConfig` 缺少 `cache` 字段或 `CacheDefaultMode` 未定义。

- [ ] **Step 3: 实现启动配置 schema 与 `AppState` 字段**

在 `aisix/crates/aisix-config/src/startup.rs` 增加：

```rust
use anyhow::Result;
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct StartupConfig {
    pub server: ServerConfig,
    pub etcd: EtcdConfig,
    pub redis: RedisConfig,
    pub log: LogConfig,
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub cache: CacheConfig,
    pub deployment: DeploymentConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CacheConfig {
    #[serde(default)]
    pub default: CacheDefaultMode,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            default: CacheDefaultMode::Disabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
pub enum CacheDefaultMode {
    #[serde(rename = "enabled")]
    Enabled,
    #[default]
    #[serde(rename = "disabled")]
    Disabled,
}
```

在 `aisix/crates/aisix-core/src/app_state.rs` 修改结构体与构造函数签名：

```rust
#[derive(Debug, Clone)]
pub struct AppState {
    pub snapshot: Arc<ArcSwap<CompiledSnapshot>>,
    pub ready: bool,
    _watcher: Option<SnapshotWatcher>,
    pub cache: MemoryCache,
    pub default_cache_enabled: bool,
    pub rate_limits: RateLimitService,
    pub usage_recorder: UsageRecorder,
}

impl AppState {
    pub fn new(
        snapshot: Arc<ArcSwap<CompiledSnapshot>>,
        ready: bool,
        default_cache_enabled: bool,
    ) -> Self {
        Self::with_redis_and_watcher(snapshot, ready, default_cache_enabled, None, None)
    }

    pub fn with_redis(
        snapshot: Arc<ArcSwap<CompiledSnapshot>>,
        ready: bool,
        default_cache_enabled: bool,
        redis: Option<RedisPool>,
    ) -> Self {
        Self::with_redis_and_watcher(snapshot, ready, default_cache_enabled, redis, None)
    }

    pub fn with_redis_and_watcher(
        snapshot: Arc<ArcSwap<CompiledSnapshot>>,
        ready: bool,
        default_cache_enabled: bool,
        redis: Option<RedisPool>,
        watcher: Option<SnapshotWatcher>,
    ) -> Self {
        Self {
            snapshot,
            ready,
            _watcher: watcher,
            cache: MemoryCache::default(),
            default_cache_enabled,
            rate_limits: RateLimitService::new(redis.clone()),
            usage_recorder: UsageRecorder::new(redis),
        }
    }
}
```

在 `aisix/crates/aisix-runtime/src/bootstrap.rs` 里读取启动配置：

```rust
let default_cache_enabled = matches!(
    config.cache.default,
    aisix_config::startup::CacheDefaultMode::Enabled
);
```

并把该布尔值传入 `AppState::with_redis_and_watcher(...)`。

更新 `aisix/config/aisix-gateway.example.yaml`：

```yaml
cache:
  default: "disabled"
```

- [ ] **Step 4: 更新 bootstrap 测试构造并重新运行**

把 `aisix/crates/aisix-runtime/tests/bootstrap.rs` 中手写的 `StartupConfig` 全部补齐：

```rust
cache: aisix_config::startup::CacheConfig {
    default: aisix_config::startup::CacheDefaultMode::Disabled,
},
```

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config startup_config -- --nocapture`
Expected: PASS

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-runtime bootstrap -- --nocapture`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add aisix/crates/aisix-config/src/startup.rs aisix/config/aisix-gateway.example.yaml aisix/crates/aisix-core/src/app_state.rs aisix/crates/aisix-runtime/src/bootstrap.rs aisix/crates/aisix-config/tests/startup_config.rs aisix/crates/aisix-runtime/tests/bootstrap.rs
git commit -m "feat: add global default cache setting"
```

### Task 2: 为 provider/model 增加三态缓存 schema 并编译进快照

**Files:**
- Modify: `aisix/crates/aisix-config/src/etcd_model.rs`
- Modify: `aisix/crates/aisix-config/src/snapshot.rs`
- Modify: `aisix/crates/aisix-config/src/compile.rs`
- Test: `aisix/crates/aisix-config/tests/snapshot_compile.rs`
- Test: `aisix/crates/aisix-config/tests/etcd_loader.rs`
- Test: `aisix/crates/aisix-config/tests/etcd_watch.rs`

- [ ] **Step 1: 写失败测试，锁定 cache 三态编译行为**

在 `aisix/crates/aisix-config/tests/snapshot_compile.rs` 增加测试：

```rust
use aisix_config::etcd_model::{
    ApiKeyConfig, CacheMode, CachePolicyConfig, ModelConfig, PolicyConfig, ProviderAuth,
    ProviderConfig, ProviderKind, RateLimitConfig,
};

#[test]
fn compile_snapshot_defaults_missing_cache_to_inherit() {
    let report = compile_snapshot(
        vec![provider()],
        vec![model()],
        vec![api_key()],
        vec![policy()],
        1,
    )
    .expect("snapshot should compile");

    assert_eq!(
        report.snapshot.provider_cache_modes.get("provider-1"),
        Some(&CacheMode::Inherit)
    );
    assert_eq!(
        report.snapshot.model_cache_modes.get("gpt-4o-mini"),
        Some(&CacheMode::Inherit)
    );
}

#[test]
fn compile_snapshot_records_explicit_cache_modes() {
    let mut provider = provider();
    provider.cache = Some(CachePolicyConfig {
        mode: CacheMode::Enabled,
    });

    let mut model = model();
    model.cache = Some(CachePolicyConfig {
        mode: CacheMode::Disabled,
    });

    let report = compile_snapshot(vec![provider], vec![model], vec![], vec![policy()], 1)
        .expect("snapshot should compile");

    assert_eq!(
        report.snapshot.provider_cache_modes.get("provider-1"),
        Some(&CacheMode::Enabled)
    );
    assert_eq!(
        report.snapshot.model_cache_modes.get("gpt-4o-mini"),
        Some(&CacheMode::Disabled)
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config snapshot_compile -- --nocapture`
Expected: FAIL，提示未知字段 `cache`、未知类型 `CacheMode` 等。

- [ ] **Step 3: 实现 provider/model 的 cache schema**

在 `aisix/crates/aisix-config/src/etcd_model.rs` 增加：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CacheMode {
    #[serde(rename = "inherit")]
    Inherit,
    #[serde(rename = "enabled")]
    Enabled,
    #[serde(rename = "disabled")]
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachePolicyConfig {
    pub mode: CacheMode,
}
```

并扩展 `ModelConfig` / `ProviderConfig`：

```rust
pub struct ModelConfig {
    pub id: String,
    pub provider_id: String,
    pub upstream_model: String,
    pub policy_id: Option<String>,
    pub rate_limit: Option<RateLimitConfig>,
    pub cache: Option<CachePolicyConfig>,
}

pub struct ProviderConfig {
    pub id: String,
    pub kind: ProviderKind,
    pub base_url: String,
    pub auth: ProviderAuth,
    pub policy_id: Option<String>,
    pub rate_limit: Option<RateLimitConfig>,
    pub cache: Option<CachePolicyConfig>,
}
```

- [ ] **Step 4: 把资源级缓存模式编译进 `CompiledSnapshot`**

在 `aisix/crates/aisix-config/src/snapshot.rs` 扩展：

```rust
use crate::etcd_model::{
    ApiKeyConfig, CacheMode, ModelConfig, PolicyConfig, ProviderConfig, RateLimitConfig,
};

pub struct CompiledSnapshot {
    pub revision: i64,
    pub keys_by_token: HashMap<String, KeyMeta>,
    pub apikeys_by_id: HashMap<String, ApiKeyConfig>,
    pub providers_by_id: HashMap<String, ProviderConfig>,
    pub models_by_name: HashMap<String, ModelConfig>,
    pub policies_by_id: HashMap<String, PolicyConfig>,
    pub provider_limits: HashMap<String, ResolvedLimits>,
    pub model_limits: HashMap<String, ResolvedLimits>,
    pub key_limits: HashMap<String, ResolvedLimits>,
    pub provider_cache_modes: HashMap<String, CacheMode>,
    pub model_cache_modes: HashMap<String, CacheMode>,
}
```

在 `aisix/crates/aisix-config/src/compile.rs` 中：

```rust
let mut provider_cache_modes = HashMap::new();
for provider in providers {
    if let Some(reason) = missing_policy_reason(provider.policy_id.as_deref(), &policies_by_id) {
        issues.push(CompileIssue {
            kind: "provider",
            id: provider.id,
            reason,
        });
        continue;
    }

    provider_cache_modes.insert(
        provider.id.clone(),
        provider
            .cache
            .as_ref()
            .map(|cache| cache.mode)
            .unwrap_or(crate::etcd_model::CacheMode::Inherit),
    );

    provider_limits.insert(
        provider.id.clone(),
        resolve_limits(
            provider.rate_limit.as_ref(),
            provider.policy_id.as_deref(),
            &policies_by_id,
        )?,
    );
    providers_by_id.insert(provider.id.clone(), provider);
}

let mut model_cache_modes = HashMap::new();
for model in models {
    if !providers_by_id.contains_key(&model.provider_id) {
        issues.push(CompileIssue {
            kind: "model",
            id: model.id,
            reason: format!("missing provider reference: {}", model.provider_id),
        });
        continue;
    }

    if let Some(reason) = missing_policy_reason(model.policy_id.as_deref(), &policies_by_id) {
        issues.push(CompileIssue {
            kind: "model",
            id: model.id,
            reason,
        });
        continue;
    }

    model_cache_modes.insert(
        model.id.clone(),
        model
            .cache
            .as_ref()
            .map(|cache| cache.mode)
            .unwrap_or(crate::etcd_model::CacheMode::Inherit),
    );

    model_limits.insert(
        model.id.clone(),
        resolve_limits(
            model.rate_limit.as_ref(),
            model.policy_id.as_deref(),
            &policies_by_id,
        )?,
    );
    models_by_name.insert(model.id.clone(), model);
}
```

并在 `CompiledSnapshot` 构造时写入两个新 map。

- [ ] **Step 5: 更新所有 fixture builder 与 JSON fixture**

在 `snapshot_compile.rs` 的 `provider()` / `model()` builder 中补：

```rust
cache: None,
```

把 `etcd_loader.rs`、`etcd_watch.rs`、`bootstrap.rs`、`admin_reload.rs` 等 JSON fixture 中的 `provider` / `model` 文本补成：

```json
"policy_id": null,
"rate_limit": null,
"cache": null
```

或在需要测试默认兼容性的位置保留字段缺失，但至少要保证断言和反序列化意图一致。

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config snapshot_compile -- --nocapture`
Expected: PASS

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config etcd_loader -- --nocapture`
Expected: PASS

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config etcd_watch -- --nocapture`
Expected: PASS

- [ ] **Step 7: 提交**

```bash
git add aisix/crates/aisix-config/src/etcd_model.rs aisix/crates/aisix-config/src/snapshot.rs aisix/crates/aisix-config/src/compile.rs aisix/crates/aisix-config/tests/snapshot_compile.rs aisix/crates/aisix-config/tests/etcd_loader.rs aisix/crates/aisix-config/tests/etcd_watch.rs
git commit -m "feat: compile provider and model cache modes"
```

### Task 3: 实现运行时缓存开关解析与 header 语义

**Files:**
- Modify: `aisix/crates/aisix-server/src/pipeline/cache.rs`
- Modify: `aisix/crates/aisix-server/src/handlers/chat.rs`
- Modify: `aisix/crates/aisix-server/src/pipeline/stream_chunk.rs`
- Test: `aisix/crates/aisix-server/tests/chat_non_stream.rs`

- [ ] **Step 1: 写失败测试，锁定运行时优先级行为**

在 `aisix/crates/aisix-server/tests/chat_non_stream.rs` 增加测试 helper 和场景：

```rust
#[tokio::test]
async fn non_stream_chat_skips_cache_when_globally_disabled_and_resources_inherit() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let state = test_state(snapshot_for_upstream(&upstream.base_url), true, false);
    let app = aisix_server::app::build_router(state);

    let first = app.clone().oneshot(chat_request()).await.unwrap();
    let second = app.oneshot(chat_request()).await.unwrap();

    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);
    assert!(first.headers().get("x-aisix-cache-hit").is_none());
    assert!(second.headers().get("x-aisix-cache-hit").is_none());
    assert_eq!(capture.hits(), 2);
}

#[tokio::test]
async fn non_stream_chat_hits_cache_when_globally_enabled_and_resources_inherit() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let state = test_state(snapshot_for_upstream(&upstream.base_url), true, true);
    let app = aisix_server::app::build_router(state);

    let first = app.clone().oneshot(chat_request()).await.unwrap();
    let second = app.oneshot(chat_request()).await.unwrap();

    assert_eq!(
        first.headers().get("x-aisix-cache-hit").and_then(|v| v.to_str().ok()),
        Some("false")
    );
    assert_eq!(
        second.headers().get("x-aisix-cache-hit").and_then(|v| v.to_str().ok()),
        Some("true")
    );
    assert_eq!(capture.hits(), 1);
}

#[tokio::test]
async fn model_cache_mode_overrides_provider_cache_mode() {
    let capture = Arc::new(CapturedRequest::default());
    let upstream = spawn_openai_mock(capture.clone()).await;

    let state = test_state(
        snapshot_for_cache_modes(
            &upstream.base_url,
            Some(aisix_config::etcd_model::CacheMode::Enabled),
            Some(aisix_config::etcd_model::CacheMode::Disabled),
        ),
        true,
        false,
    );
    let app = aisix_server::app::build_router(state);

    let first = app.clone().oneshot(chat_request()).await.unwrap();
    let second = app.oneshot(chat_request()).await.unwrap();

    assert!(first.headers().get("x-aisix-cache-hit").is_none());
    assert!(second.headers().get("x-aisix-cache-hit").is_none());
    assert_eq!(capture.hits(), 2);
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server chat_non_stream -- --nocapture`
Expected: FAIL，提示 `test_state` 参数不匹配、`snapshot_for_cache_modes` 不存在、当前 header 行为与预期不一致。

- [ ] **Step 3: 在 `pipeline/cache.rs` 收口最终缓存决策**

在 `aisix/crates/aisix-server/src/pipeline/cache.rs` 增加统一函数：

```rust
use aisix_config::etcd_model::CacheMode;

pub fn cache_enabled_for_chat(
    ctx: &RequestContext,
    state: &ServerState,
) -> Result<bool, GatewayError> {
    let CanonicalRequest::Chat(chat_request) = &ctx.request else {
        return Ok(false);
    };

    if chat_request.stream {
        return Ok(false);
    }

    let model_mode = ctx
        .snapshot
        .model_cache_modes
        .get(&chat_request.model)
        .copied()
        .unwrap_or(CacheMode::Inherit);

    match model_mode {
        CacheMode::Enabled => return Ok(true),
        CacheMode::Disabled => return Ok(false),
        CacheMode::Inherit => {}
    }

    let provider_id = ctx
        .resolved_provider_id
        .as_deref()
        .ok_or_else(|| GatewayError {
            kind: ErrorKind::Internal,
            message: "resolved provider missing before cache decision".to_string(),
        })?;

    let provider_mode = ctx
        .snapshot
        .provider_cache_modes
        .get(provider_id)
        .copied()
        .unwrap_or(CacheMode::Inherit);

    match provider_mode {
        CacheMode::Enabled => Ok(true),
        CacheMode::Disabled => Ok(false),
        CacheMode::Inherit => Ok(state.app.default_cache_enabled),
    }
}
```

- [ ] **Step 4: 把读缓存和写缓存都切到统一决策函数**

在 `aisix/crates/aisix-server/src/handlers/chat.rs` 中改成：

```rust
if ctx.request.transport_mode() == TransportMode::Json && cache::cache_enabled_for_chat(&ctx, &state)? {
    if let Some(response) = cache::lookup_chat(&mut ctx, &state)? {
        post_call::record_success(&ctx, &state).await;
        return Ok(response);
    }
}
```

在 `aisix/crates/aisix-server/src/pipeline/stream_chunk.rs` 中改成：

```rust
let cache_enabled = cache::cache_enabled_for_chat(ctx, state)?;

if output.status.is_success() {
    ctx.usage = output.usage.clone();
    if cache_enabled
        && matches!(&ctx.request, CanonicalRequest::Chat(chat_request) if !chat_request.stream)
    {
        cache::store_chat_success(ctx, state, output.body.as_ref(), output.usage.clone())?;
    }
}

let cache_hit = match &ctx.request {
    CanonicalRequest::Chat(_) if cache_enabled => Some("false"),
    CanonicalRequest::Chat(_) => None,
    CanonicalRequest::Embeddings(_) => None,
};
```

这里允许 `lookup_chat()` / `store_chat_success()` 保持原职责，不再自行解析是否启用缓存。

- [ ] **Step 5: 更新测试 helper，让 `AppState` 接收默认缓存值**

把 `chat_non_stream.rs` 的 `test_state` 改成：

```rust
fn test_state(
    snapshot: CompiledSnapshot,
    ready: bool,
    default_cache_enabled: bool,
) -> aisix_server::app::ServerState {
    aisix_server::app::ServerState {
        app: AppState::new(initial_snapshot_handle(snapshot), ready, default_cache_enabled),
        providers: ProviderRegistry::default(),
        admin: None,
    }
}
```

增加 `snapshot_for_cache_modes()` helper，在 provider/model fixture 中注入：

```rust
fn snapshot_for_cache_modes(
    base_url: &str,
    provider_mode: Option<aisix_config::etcd_model::CacheMode>,
    model_mode: Option<aisix_config::etcd_model::CacheMode>,
) -> CompiledSnapshot {
    let mut snapshot = snapshot_for_upstream(base_url);

    if let Some(provider) = snapshot.providers_by_id.get_mut("openai") {
        provider.cache = provider_mode.map(|mode| aisix_config::etcd_model::CachePolicyConfig { mode });
    }

    if let Some(model) = snapshot.models_by_name.get_mut("gpt-4o-mini") {
        model.cache = model_mode.map(|mode| aisix_config::etcd_model::CachePolicyConfig { mode });
    }

    snapshot.provider_cache_modes.insert(
        "openai".to_string(),
        provider_mode.unwrap_or(aisix_config::etcd_model::CacheMode::Inherit),
    );
    snapshot.model_cache_modes.insert(
        "gpt-4o-mini".to_string(),
        model_mode.unwrap_or(aisix_config::etcd_model::CacheMode::Inherit),
    );

    snapshot
}
```

- [ ] **Step 6: 运行测试确认通过**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server chat_non_stream -- --nocapture`
Expected: PASS

- [ ] **Step 7: 提交**

```bash
git add aisix/crates/aisix-server/src/pipeline/cache.rs aisix/crates/aisix-server/src/handlers/chat.rs aisix/crates/aisix-server/src/pipeline/stream_chunk.rs aisix/crates/aisix-server/tests/chat_non_stream.rs
git commit -m "feat: resolve cache behavior from model provider and default"
```

### Task 4: 修复全仓测试构造与 Admin/Loader 兼容性

**Files:**
- Modify: `aisix/crates/aisix-runtime/tests/bootstrap.rs`
- Modify: `aisix/crates/aisix-config/tests/etcd_loader.rs`
- Modify: `aisix/crates/aisix-config/tests/etcd_watch.rs`
- Modify: `aisix/crates/aisix-server/tests/rate_limit.rs`
- Modify: `aisix/crates/aisix-server/tests/admin_reload.rs`

- [ ] **Step 1: 写出本任务目标清单**

本任务不新增业务逻辑，目标是把所有因 schema 和构造签名变化而失败的测试恢复为绿色。需要检查：

```text
1. 所有 StartupConfig 手写构造都补 cache 默认值
2. 所有 AppState::new(...) 调用都补 default_cache_enabled
3. 所有 ProviderConfig / ModelConfig 构造都补 cache: None
4. 所有 provider/model JSON fixture 都补 cache: null 或明确保留缺失作为兼容性测试
```

- [ ] **Step 2: 运行目标测试，收集失败点**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_reload rate_limit -- --nocapture`
Expected: FAIL，显示结构体字段缺失或函数签名不匹配。

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config etcd_loader etcd_watch -- --nocapture`
Expected: FAIL，显示 JSON fixture 或 `CompiledSnapshot` 初始化缺字段。

- [ ] **Step 3: 修复所有 helper 和 fixture**

按以下模式统一补齐：

`StartupConfig`：

```rust
cache: aisix_config::startup::CacheConfig {
    default: aisix_config::startup::CacheDefaultMode::Disabled,
},
```

`ProviderConfig`：

```rust
cache: None,
```

`ModelConfig`：

```rust
cache: None,
```

`CompiledSnapshot`：

```rust
provider_cache_modes: Default::default(),
model_cache_modes: Default::default(),
```

`AppState::new(...)`：

```rust
AppState::new(initial_snapshot_handle(snapshot), true, false)
```

- [ ] **Step 4: 重新运行受影响测试**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server admin_reload rate_limit -- --nocapture`
Expected: PASS

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config etcd_loader etcd_watch -- --nocapture`
Expected: PASS

- [ ] **Step 5: 提交**

```bash
git add aisix/crates/aisix-runtime/tests/bootstrap.rs aisix/crates/aisix-config/tests/etcd_loader.rs aisix/crates/aisix-config/tests/etcd_watch.rs aisix/crates/aisix-server/tests/rate_limit.rs aisix/crates/aisix-server/tests/admin_reload.rs
git commit -m "test: update helpers for cache config changes"
```

### Task 5: 更新产品文档

**Files:**
- Modify: `aisix/README.md`
- Modify: `aisix/docs/admin-api.md`

- [ ] **Step 1: 写出要同步的事实清单**

产品文档必须同步这 6 个事实：

```text
1. 启动 YAML 支持 cache.default
2. 默认全局策略是 disabled
3. provider/model 支持 cache.mode
4. cache.mode 三态为 inherit/enabled/disabled
5. 不配置等于 inherit
6. 运行时优先级为 model > provider > global default
```

- [ ] **Step 2: 更新 README 示例与说明**

在 `aisix/README.md` 添加中文或现有文档风格一致的说明，至少包含：

```md
## Cache Policy

- Global default cache behavior is configured in `aisix/config/aisix-gateway.example.yaml` via `cache.default`.
- Supported values: `enabled`, `disabled`.
- Provider/model resources may set `cache.mode` to `inherit`, `enabled`, or `disabled`.
- Missing `cache` is treated as `inherit`.
- Effective precedence is `model > provider > global default`.
- Current response caching applies only to non-stream chat JSON requests.
```

- [ ] **Step 3: 更新 Admin API 文档中的 provider/model schema 示例**

在 `aisix/docs/admin-api.md` 的 provider/model 示例中补：

```json
"cache": { "mode": "enabled" }
```

并说明字段可选，缺失时按 `inherit`。

- [ ] **Step 4: 在产品文档中明确优先级与边界**

在 `aisix/README.md` 和 `aisix/docs/admin-api.md` 中明确：

```md
- 启动 YAML 通过 `cache.default` 提供全局默认缓存策略
- provider/model 通过 `cache.mode` 提供资源级覆盖
- 最终优先级为 `model > provider > global default`
- 当前不支持 key 级静态缓存开关
- 当前不把缓存配置挂在 policy 上
```

- [ ] **Step 5: 运行基础文档相关验证**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config startup_config -- --nocapture`
Expected: PASS，确认示例 YAML 仍可加载。

- [ ] **Step 6: 提交**

```bash
git add aisix/README.md aisix/docs/admin-api.md
git commit -m "docs: document cache policy configuration"
```

### Task 6: 反向更新 `.agents` 架构 skill

**Files:**
- Modify: `.agents/skills/arch/infra/sections/config.md`
- Modify: `.agents/skills/arch/data-model/sections/entities.md`
- Modify: `.agents/skills/arch/data-model/sections/compilation.md`

- [ ] **Step 1: 明确本任务目标**

本任务不是普通文档补充，而是把已经确认的架构事实反向同步到 agent 使用的架构 skill，确保后续任何 agent 基于 skill 获取到的架构信息与代码和正式文档一致。必须同步这 7 个事实：

```text
1. 启动 YAML 支持 cache.default
2. 默认全局策略是 disabled
3. provider/model 支持 cache.mode
4. cache.mode 三态为 inherit/enabled/disabled
5. 不配置等于 inherit
6. 运行时优先级为 model > provider > global default
7. agent 架构 skill 必须随架构变更保持同步
```

- [ ] **Step 2: 更新 infra skill 的启动配置 section**

在 `.agents/skills/arch/infra/sections/config.md` 把启动配置示例更新为：

```yaml
server:
  listen: "0.0.0.0:4000"
  metrics_listen: "0.0.0.0:9090"
  request_body_limit_mb: 8
etcd:
  endpoints:
    - "http://127.0.0.1:2379"
  prefix: "/aisix"
  dial_timeout_ms: 5000
redis:
  url: "redis://127.0.0.1:6379"
log:
  level: "info"
runtime:
  worker_threads: 1
cache:
  default: "disabled"
deployment:
  admin:
    enabled: true
    admin_keys:
      - key: "change-me-admin-key"
```

并在该 section 末尾增加一句说明：

```md
`cache.default` 提供网关进程级默认缓存策略；资源级覆盖不写入启动配置。
```

- [ ] **Step 3: 更新 data-model skill 的实体 schema section**

在 `.agents/skills/arch/data-model/sections/entities.md` 的 `Provider` / `Model` 示例中补 `cache.mode`，并明确：

```md
- `cache.mode` 可选值：`inherit`、`enabled`、`disabled`
- 未配置 `cache` 视为 `inherit`
- `apikey` 与 `policy` 当前不承载缓存开关
```

- [ ] **Step 4: 更新 data-model skill 的编译规则 section**

在 `.agents/skills/arch/data-model/sections/compilation.md` 增加：

```md
5. 缓存模式编译 → provider/model 的 `cache.mode` 编译为资源级快照映射；缺失时视为 `inherit`

运行时最终缓存开关不只取决于快照，还会结合启动配置中的全局默认值，按 `model > provider > global default` 解析。
```

- [ ] **Step 5: 自检 skill 与正式文档是否一致**

人工核对以下三处的一致性：

```text
1. docs/superpowers/plans/2026-04-10-aisix-cache-policy.md
2. aisix/README.md 与 aisix/docs/admin-api.md
3. .agents/skills/arch/infra + arch/data-model 对应 section
```

检查点：术语一致、优先级一致、默认值一致、边界一致（不支持 key / 不挂 policy）。

- [ ] **Step 6: 提交**

```bash
git add .agents/skills/arch/infra/sections/config.md .agents/skills/arch/data-model/sections/entities.md .agents/skills/arch/data-model/sections/compilation.md
git commit -m "docs: sync cache architecture into agent skills"
```

### Task 7: 全量验证

**Files:**
- Modify: 无
- Test: 全仓关键测试与 lint

- [ ] **Step 1: 运行配置与服务相关测试**

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-config`
Expected: PASS

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-runtime`
Expected: PASS

Run: `cargo test --manifest-path aisix/Cargo.toml -p aisix-server`
Expected: PASS

- [ ] **Step 2: 运行全仓测试**

Run: `cargo test --manifest-path aisix/Cargo.toml`
Expected: PASS

- [ ] **Step 3: 运行 clippy**

Run: `cargo clippy --manifest-path aisix/Cargo.toml -- -D warnings`
Expected: PASS

- [ ] **Step 4: 提交最终验证状态**

```bash
git status
```

Expected: working tree clean，或只剩用户明确保留的未提交更改。

- [ ] **Step 5: 提交**

```bash
git add .
git commit -m "feat: add configurable cache policy resolution"
```

## 自检

### 需求覆盖

- 全局默认缓存开关：由 Task 1 覆盖。
- provider/model 三态缓存策略：由 Task 2 覆盖。
- 不配置等于 `inherit`：由 Task 2 和 Task 5 覆盖。
- 运行时优先级 `model > provider > global default`：由 Task 3 覆盖。
- 不支持 key 级静态开关：通过 Task 2 的 schema 边界和 Task 5/6 文档边界覆盖。
- `.agents` 文档同步：由 Task 6 覆盖。
- 架构变更后反向同步 skill：由 Task 6 的独立目标和自检覆盖。

### 占位符扫描

- 文档中没有未完成占位语句。
- 每个修改步骤都给出了具体文件、代码片段和命令。

### 类型一致性

- 启动配置使用 `CacheDefaultMode::{Enabled, Disabled}`。
- 资源配置使用 `CacheMode::{Inherit, Enabled, Disabled}`。
- 快照字段统一命名为 `provider_cache_modes` / `model_cache_modes`。
- `AppState` 统一持有 `default_cache_enabled: bool`。
