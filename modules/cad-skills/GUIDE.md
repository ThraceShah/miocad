# 通用 Agent CAD 使用指南

适用于任意能调用 CLI 或 MCP stdio 的 Agent，无需专用模型或 GUI。
Rust CLI：`cad-cli`，逐行发送 JSON；Rust MCP：`cad-mcp`，工具 cad_command，参数 command 包装相同 JSON。
两者默认附着共享服务。同一 runtime-dir 的其他进程可继续使用已有文档；入口退出不会丢弃文档。
安装、参数和原 Python 配置替换见 [P4 迁移指南](../../docs/migration-p4.md)。

1. 调用 commands.list 发现统一命令、参数和前置条件；capabilities 查询核心能力，不能假设 GPU 已实现。
2. 先用 service.snapshot 或 document.list 找到已有文档，或 document.create 创建，保存 ID 和 revision。
   尺寸/半径为 mm，体积为 mm³；没有全局“当前文档”，每次建模显式指定 ID。
3. 每次修改携带当前 revision；成功后使用新 revision。revision_conflict 应刷新状态并重新决定操作。
4. render.snapshot 返回普通 shaded 和两张 ID 图（images 值为 base64 PNG），保存 render_id 和尺寸。
   使用 shaded 图定位像素，再用同一 render_id 调用 render.pick。坐标原点左上，x 向右，y 向下。
5. 检查 reference 和 ambiguous。背景或歧义不能直接建模，换邻近像素或用 topology.edges 的几何查询消歧。
   可选 target=face；Fillet 当前只接受 edge 引用。不要修改 opaque reference 字符串。
6. model.fillet 提供 document/revision/edge 和正半径。任何编辑、Undo/Redo 后都重新截图或查询引用；
   stale_render、invalid_reference 都要求重新选择，不能复用旧图中的 slot。
7. 收到明确业务失败后核对 document.state，降低半径或重新选择。收到 outcome_unknown 时不要盲目重做，
   原请求位于 error.request：同一逻辑客户端 service.reconnect，然后 request.status 查询其 request_id，
   必要时读取快照。结果缓存过期不代表原操作从未执行。服务/worker 已变更则不能迁移旧请求。
8. 检查 valid、volume、拓扑摘要，再取结果截图。valid 是本原型的 AssertValid + manifold + 正有限体积检查，
   不等于完整制造可行性证明。export.native 写 .smb；父目录需已存在。不提供 STEP 或备用 OCCT 内核。
9. 用 service.events(after) 拉变化；初次/重连或 resync_required 时取 service.snapshot，以其 cursor 为准。
   会话可通过 service.sessions 找到、service.attach 切换；不要将隔离会话中的帧用于默认会话。

当前限制：每文档单 body、单边 Fillet、CPU 近似拾取、内存历史、无 Persistent Naming/磁盘恢复。
默认 384×384，图片尺寸可为 64..768；每会话保留最近四帧，淘汰后需重拍。
服务身份/请求缓存/资源配额详见服务文档；不提供任意内核代码执行。
场景截图无窗口也可运行，GUI 窗口截图和交互属于后续阶段。

显式 service.shutdown 会停止整个共享实例并丢弃内存文档，不是关闭单个入口；先完成所需原生导出。
Rust 示例：`cargo run -p cad-access --example box_fillet --locked -- --runtime-dir /tmp/cad-demo`。
