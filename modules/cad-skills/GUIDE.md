# 通用 Agent CAD 使用指南

本指南适用于任何能调用 CLI 或 MCP stdio 的 Agent，无需专用模型。

1. 调用 capabilities。先确认所需功能，不能假设 GPU 已实现。
2. 创建 document，保存 ID 和 revision；尺寸和半径使用 mm。
3. 每次修改提供当前 revision，成功后采用响应中的新 revision。
4. 视觉选择：render.snapshot 获取普通 shaded 图，确定目标像素，再用同一个 render_id 调用
   render.pick。检查返回 reference 是否为 null；必要时结合 topology.edges 的几何折线和长度。
5. reference 绑定当前文档版本。发生任何成功编辑、undo 或 redo 后，重新截图/查询/拾取。
6. model.fillet 提供单条边的 reference 和正半径；失败后读取 document.state，降低半径或重新选择。
   不要把失败当成成功，也不要通过修改 reference 的字符串来选择其他实体。
7. 检查 valid、volume 和拓扑摘要，再请求结果截图。valid 表示本原型的 AssertValid + manifold +
   正有限体积检查，不等于完整制造可行性证明。
8. 当前仅支持 export.native 写 SMB。STEP 导出不属于项目要求。

CLI 会话：`python3 modules/cad-cli/main.py`，持续逐行发送 JSON。
MCP 会话：`python3 modules/cad-mcp/server.py`，调用工具 cad_command，参数 command 包装同样 JSON。
两个入口都复用 scripts/client.py 和独立 C++ worker，几何逻辑没有分叉。
批量建模需要保留同一个进程；不要为每条命令启动一个新会话。

原型限制：单文档单 body、单边圆角、CPU 拾取、内存历史、无持久命名；
不提供任意内核代码执行工具。可使用 examples/box_fillet.py 验证安装和真实内核能力。
