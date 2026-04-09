# 启动序列

1. 解析 YAML 启动配置
2. 连接 etcd，GET 完整前缀范围（捕获 revision N）
3. 解析 secret 引用（`env:KEY` → 实际值）
4. 执行三层校验（schema → 语义 → 运行时预检）
5. `compile_snapshot()` 产出编译报告并发布可用快照
6. `ArcSwap.store()` — 网关开始服务
7. 从 revision N+1 启动后台 watcher

配置编译的详细规则（哪些资源会被跳过、哪些错误会阻止发布）统一以 `arch-data-model` 为准。
