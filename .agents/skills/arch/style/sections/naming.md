# 命名规范

### Crate

- 所有 crate 使用 `aisix-` 前缀：`aisix-types`、`aisix-config`、`aisix-auth`
- `Cargo.toml` 中用连字符，Rust 代码中用下划线：`aisix_types`、`aisix_config`

### 类型

- **PascalCase** 用于类型、结构体、枚举、trait
- 配置结构体用 `Config` 后缀：`RateLimitConfig`、`ProviderConfig`
- 运行时/计算结果用纯名词：`ResolvedLimits`、`ResolvedTarget`
- "类别"枚举用 `Kind` 后缀：`ErrorKind`、`ProviderKind`
- 枚举变体用 PascalCase，包括缩写：`OpenAi`（不是 `OpenAI`）

### 字段

- 全部 **snake_case**：`key_id`、`upstream_model`、`provider_id`
- HashMap 查找字段用 `_by_` 模式：`keys_by_token`、`models_by_name`、`providers_by_id`
- ID 字段用 `_id` 后缀：`key_id`、`provider_id`、`policy_id`
- 可选字段直接用 `Option<T>`，不加 `opt_` 前缀：`expires_at: Option<DateTime<Utc>>`

### 函数

- **snake_case**：`compile_snapshot`、`resolve_limits`、`bearer_token`
- 构造函数：`new()` 为主构造器，`with_*` 为变体
- 返回错误的辅助函数用名词/谓语命名：`invalid_api_key() -> GatewayError`
