# 测试

- 集成测试放在 `crates/*/tests/`，使用 `#[tokio::test]`
- 描述性 snake_case 命名：`maps_too_many_requests_to_rate_limit_error`
- 直接构造类型，不使用测试 fixture 或 builder
- 共享 `TestApp::start()` 辅助函数用于全栈测试
- 外部依赖通过 wiremock + docker-compose 提供
