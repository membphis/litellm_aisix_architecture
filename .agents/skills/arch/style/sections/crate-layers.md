# Crate 依赖层级

```
L0 基础层：     aisix-types              （无内部依赖）
L1 核心设施：   aisix-config, aisix-storage, aisix-core
L2 领域层：     aisix-auth, aisix-policy, aisix-router,
               aisix-ratelimit, aisix-cache, aisix-providers,
               aisix-spend, aisix-observability
L3 编排层：     aisix-runtime, aisix-server
L4 入口层：     aisix-gateway
```

依赖流必须严格自顶向下。L0 不能依赖 L1+。
L2 可以依赖 L0-L1，但 L2 之间不应互相依赖（除非有明确理由）。
