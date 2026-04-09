# etcd Key 布局

所有运行时配置位于可配置前缀下（默认 `/aisix`）：

```
/aisix/providers/{provider_id}     # Provider 绑定信息
/aisix/models/{model_id}           # LLM 模型定义
/aisix/apikeys/{apikey_id}         # 客户端身份
/aisix/policies/{policy_id}        # 可复用的限流策略模板
```
