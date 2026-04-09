# Trait 设计

- `ProviderCodec`：`#[async_trait]` + `Send + Sync + 'static` 约束
- 私有辅助 trait（如 `HasConfigId`）用于泛型集合逻辑
- Axum extractor 通过 `FromRequestParts` 实现，用 `FromRef` 链式提取状态
