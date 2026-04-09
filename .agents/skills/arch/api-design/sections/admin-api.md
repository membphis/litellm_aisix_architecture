# Admin API

## 认证

所有 admin 请求需要 `x-admin-key` 头与配置值匹配。缺失或无效 → 401。

## 路由（每个集合：providers、models、apikeys、policies）

| 方法 | 路径 | 说明 |
|------|------|------|
| `GET` | `/admin/<collection>` | 列出所有（按 id 升序） |
| `GET` | `/admin/<collection>/:id` | 获取单个 |
| `PUT` | `/admin/<collection>/:id` | Upsert（路径 id 必须与 body 匹配） |
| `DELETE` | `/admin/<collection>/:id` | 删除 |

## 写入语义

Admin 写入先到 **etcd**。后台 watcher 异步应用变更。
成功的 PUT/DELETE 意味着 etcd 接受了写入，并不代表运行时快照已更新。
当前资源如果依赖无效，可能暂时不会出现在运行时快照中；其他有效资源继续收敛。详细编译语义见 `arch-data-model`。

写入响应格式：
```json
{ "id": "openai", "path": "/aisix/providers/openai", "revision": 123 }
```

允许乱序写入（例如先写 model 再写其 provider）。收敛在 watcher 层完成；依赖补齐后，后续 reload 会自动纳入该资源。

## 错误码

| HTTP | 条件 |
|------|------|
| 401 | 缺失/无效 admin key |
| 400 | 无效 id 或路径/body 不匹配 |
| 404 | 资源不存在（GET/DELETE） |
| 500 | etcd 或服务器故障 |
