# 认证与鉴权

## Authentication（认证）

- 自定义 Axum extractor `AuthenticatedKey`，通过 `FromRequestParts` 实现
- 从 `Authorization` 头提取 `Bearer` token
- 在 `CompiledSnapshot.keys_by_token` 中 O(1) HashMap 查找
- 检查 `expires_at`；过期 key 返回 401
- 将 `KeyMeta` 注入 pipeline 上下文

## Authorization（鉴权）

- 检查已认证 key 的 `allowed_models` 是否包含请求的 model
- 使用 `*` 通配符允许所有模型
- 不在允许列表中返回 403 `permission_denied`
