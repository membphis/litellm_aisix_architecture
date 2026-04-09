# 序列化

- 所有配置/数据类型同时派生 `Serialize, Deserialize`
- `#[serde(rename = "...")]` 用于线上格式的名称映射和 Rust 保留字
- `#[serde(default)]` 用于向后兼容的可选字段
- `GatewayError` 手动构造 JSON 响应（不派生 `Serialize`）
