# 错误响应格式

错误 envelope 由客户端入口协议决定：

- OpenAI 兼容端点（如 `/v1/chat/completions`、`/v1/embeddings`）返回 OpenAI 兼容 JSON。
- Anthropic Messages 端点（`/v1/messages`）返回 Anthropic error envelope。

OpenAI error 示例：

```json
{
  "error": {
    "message": "Invalid API key provided",
    "type": "authentication_error",
    "code": "invalid_api_key"
  }
}
```

Anthropic error 示例：

```json
{
  "type": "error",
  "error": {
    "type": "invalid_request_error",
    "message": "missing required anthropic-version header"
  }
}
```

## ErrorKind → HTTP 状态码 + error.type 映射

OpenAI 兼容端点与 Anthropic Messages 端点共享同一组 HTTP 状态码，但 `error.type` 文案可以按协议不同而不同。

| HTTP | `error.type` | ErrorKind |
|------|-------------|-----------|
| 401 | `authentication_error` | `Authentication` |
| 403 | `permission_error` / `permission_denied` | `Permission` |
| 429 | `rate_limit_error` | `RateLimited` |
| 429 | `budget_exceeded` | （费用限制） |
| 400 | `invalid_request_error` | `InvalidRequest` |
| 502 | `api_error` / `upstream_error` | `Upstream` |
| 504 | `api_error` / `timeout_error` | `Timeout` |
| 500 | `api_error` / `internal_error` | `Internal` |

## 可重试错误

`ErrorKind::retryable()` 对以下返回 true：`RateLimited`、`Timeout`、
`UpstreamUnavailable`、`Overloaded`。重试/fallback 决策基于此方法，
而非 Provider 特定的错误字符串。
