# 配置编译规则

`compile_snapshot()` 是**纯函数**（无 I/O），返回 `Result<SnapshotCompileReport, String>`。

它执行以下规则：

1. 重复 ID 检测 → 两个实体共享同一 `id` 则拒绝
2. 重复明文 API key 检测 → 两个 key 共享同一 `key` 则拒绝
3. 外键/策略引用校验 → 引用缺失时跳过当前资源并记录 `CompileIssue`
4. 限流解析 → 合并策略默认值与内联覆盖

资源级语义：

- provider 引用缺失 policy → 该 provider 跳过
- model 引用缺失 provider 或 policy → 该 model 跳过
- apikey 引用缺失 model 或 policy → 该 apikey 跳过
- 被跳过资源在当前运行时快照中视为 absent，不保留旧版本

发布语义：

- 仅存在 `CompileIssue` 时，watcher 发布有效资源子集并记录日志
- 出现硬错误（如 duplicate id / duplicate token）时，本次发布失败，旧快照继续生效
