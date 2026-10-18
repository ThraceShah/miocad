# cad-access

CLI/MCP 共用的薄应用适配层：启动参数、默认 SharedClient、neutral catalog 校验、
产品命令到 worker/service 请求的映射、结构化错误、MCP 工具 schema 和有界 stdio 读取。
不持有几何权威状态、不渲染、不启动 Python。未来 GUI 可使用 cad-client 和协议 DTO，
无需通过 CLI/MCP；此 crate 的 Frontend 是当前两种 stdio 产品入口的便利封装。

commands.list 合并 contract.json 的 commands 与 access_commands；MCP schema 将其中的本地 $ref 展开。
worker 内核心目录与客户端产品目录有意区分，服务/渲染命令不会传入 C++ Commander。

共享服务断开后不自动重放；outcome_unknown 保留 Request，同实例 reconnect + request.status 用于核对。
服务 snapshot/pick 是查询；可选文件输出失败也不改变文档历史。详见 P4 迁移指南。
