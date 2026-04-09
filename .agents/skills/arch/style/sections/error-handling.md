# 错误处理

按层级使用三种不同策略：

### 1. GatewayError（HTTP 边界，`aisix-types::error`）

扁平结构体，不嵌套，不带 backtrace：

```rust
enum ErrorKind { Authentication, Permission, NotFound, InvalidRequest, RateLimited, Timeout, Upstream, Internal }
struct GatewayError { kind: ErrorKind, message: String }
```

- 用结构体字面量显式构造，不通过 `From` 转换
- 实现 `axum::IntoResponse`，输出 OpenAI 兼容 JSON
- 所有 pipeline 阶段返回 `Result<_, GatewayError>`

### 2. RedisError（基础设施层，`aisix-storage`）

使用 `thiserror`，通过 `#[from]` 自动转换 `std::io::Error`。

### 3. Result<_, String>（配置编译）

`compile_snapshot()` 返回 `Result<SnapshotCompileReport, String>`。
错误是格式化字符串，不跨越 HTTP 边界。
编译报告字段和 skip/fail 语义以 `arch-data-model` 为准。

### 规则

- **领域代码中不使用 `anyhow`。** 仅在二进制入口的启动阶段使用。
- 绝不在 HTTP 响应中暴露内部错误细节。在 handler 层映射为 `GatewayError`。
