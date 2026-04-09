# Serde 约定

- 所有配置结构体双向可序列化（`Serialize + Deserialize`）
- `#[serde(rename = "...")]` 用于线上格式的枚举变体名：`#[serde(rename = "openai")] OpenAi`
- `#[serde(default)]` 用于向后兼容的可选字段
- `#[serde(rename = "type")]` 用于 Rust 保留字命名的字段
