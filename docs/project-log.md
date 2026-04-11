1. 完成功能全景图（面向的用户、使用者、开发者）

    * 先分析 AISIX 现有功能整理出功能全景图
    * 在继续补齐 litellm 中缺失的功能

    ```提示词
    这是 https://docs.litellm.ai/docs/ 文档站官网
    我希望获取一份 litellm 的功能全景图
    主要分析他数据面能力
    比如 model、apikey 等
    全景图需包含数据面所有功能的同时，也许给出对应经典配置示例
    这样我有这份手册就能知道目前的全景图，以及主要使用姿势
    如果需要，你也可以自动下载相关文档 github 仓库或源码仓库（毕竟他是一个开源项目）
    ```

    --> litellm-data-plane-panorama-20260403.md

2. 根据 功能全景图 ，给 AI 一个任务

    * 设计后端架构，并告诉我为什么这么选择

    ```
    我准备做一个全新的 AI Gateway 产品，名字叫 AISIX
    他是一个开源的 AI Gateway 产品，主要用来做其中的数据面部分
    这个项目，我希望使用 Rust 语言来开发
    现在我需要设计 AISIX 的后端架构
    你需要根据我提供的 litellm 的功能全景图，设计一个适合 AISIX 的后端架构
    你需要考虑到 AISIX 的目标用户、使用者和开发者的需求，以及性能、安全性、可扩展性等方面的因素
    你需要给出一个详细的后端架构设计方案，并解释为什么选择这个架构，以及它如何满足 AISIX 的需求

    litellm 的功能全景图：litellm-data-plane-panorama-20260403.md
    ```



3. 分析 AISIX 后端架构

    对比 3 和 2，他们的后端架构
    有什么不同？
    各自优缺点？
    哪个架构更适合 1 全景图

4. 得到了一个后端架构，假设新架构和当前有出入

    * 设计一个能够覆盖所有必要功能点的请求路径，比如：
        client -> auth -> model access -> rate limit -> transform -> stream|no stream -> metrics -> quota -> response client
    * 确定 AI 没说谎

5. 确定最终合适的后端架构

    * 确定最终选择哪个架构，并给出理由

6. 补充控制面实现约束（2026-04-11）

    * Admin UI 与 Admin API 强绑定，运行在同一个 admin listener / admin 端口
    * admin 端口必须与数据面端口分离，不能重叠
    * 浏览器中的 admin key 必须由用户手动输入，只保存在当前 session，关闭浏览器后丢失
    * 用户文档、架构文档、agent 指令都需要同步到这套口径，避免后续实现回退到独立 `aisix-admin` 服务假设
