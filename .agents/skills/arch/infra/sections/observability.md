# 可观测性

## 指标（Prometheus）

在指标端口暴露（推荐 9090）。核心指标：
- 按 model/provider 分类的请求延迟直方图
- Token 用量计数器（输入/输出/缓存）
- 限流命中/未命中计数器
- 缓存命中率
- 上游错误率

## 链路追踪

`tracing` crate + 结构化 span。通过 `tower_http::trace::TraceLayer`
对所有 HTTP 请求自动埋点。

## 结构化日志

每个请求输出 `UsageEvent`，structlog 格式：
- Request ID、Key ID、Model、Provider
- 输入/输出 Token、费用
- 延迟、缓存命中状态
- Phase 2+：callback sink（Langfuse、Datadog）
