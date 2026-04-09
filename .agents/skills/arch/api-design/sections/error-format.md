# 错误响应格式

所有代理错误遵循 OpenAI 兼容 JSON：

```json
{
  "error": {
    "message": "Invalid API key provided",
    "type": "authentication_error",
    "code": "invalid_api_key"
  }
}
```

## ErrorKind → HTTP 状态码 + error.type 映射

| HTTP | `error.type` | ErrorKind |
|------|-------------|-----------|
| 401 | `authentication_error` | `Authentication` |
| 403 | `permission_denied` | `Permission` |
| 429 | `rate_limit_error` | `RateLimited` |
| 429 | `budget_exceeded` | （费用限制） |
| 400 | `invalid_request_error` | `InvalidRequest` |
| 502 | `upstream_error` | `Upstream` |
| 504 | `timeout_error` | `Timeout` |
| 500 | `internal_error` | `Internal` |

## 可重试错误

`ErrorKind::retryable()` 对以下返回 true：`RateLimited`、`Timeout`、
`UpstreamUnavailable`、`Overloaded`。重试/fallback 决策基于此方法，
而非 Provider 特定的错误字符串。
