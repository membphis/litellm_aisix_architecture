# 限流解析

## 三层独立检查

每个资源（provider、model、apikey）都可以携带自己的 `rate_limit`。
三层**独立检查**，执行顺序：provider → model → apikey。
任何一层超限立即返回 429。

## 内联覆盖语义

- 资源上的内联 `rate_limit` **覆盖** `policy_id` 引用
- 仅设置 `policy_id` 时，使用策略中的限流值
- 两者都未设置时，该层**不限流**（no-op）

## Redis Key 模式

| 维度 | Key 模式 |
|------|---------|
| TPM | `rl:tpm:{scope}:{id}:{model}:{window}` |
| RPM | `rl:rpm:{scope}:{id}:{model}:{window}` |
| 并发 | `rl:cc:{scope}:{id}:{model}`（Sorted Set） |
| 冷却 | `cooldown:{target_id}` |
