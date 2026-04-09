# 当前阶段妥协

- [!NOTE] 尚无 guardrail 回调（Phase 3）。Pipeline 阶段以 no-op 存在。
- [!NOTE] 尚无 prompt template / 请求变更（Phase 2）。Pipeline 阶段以 no-op 存在。
- [!NOTE] Admin API 读取 key 时返回明文 `key` 字段。无哈希处理。
- [!NOTE] 尚无 fallback 机制（Phase 2）。RouteSelect 选择单一目标。
  首字节前 fallback 基础设施将随流式核心一起构建。
