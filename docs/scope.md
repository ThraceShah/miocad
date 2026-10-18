# 当前项目范围（2026-10-18）

按用户指示回退到引入 OCCT 之前的 SMLib MVP 阶段，并修订后续验收要求。
此文件取代此前任务中有关 STEP 导出和 OCCT 备选内核的要求。

- 构建 Agent-Agnostic、Headless-First 的精确 B-Rep CAD 系统，不绑定专用 LLM 或 Agent。
- 内核使用 SMLib；不设置 OCCT / OpenCASCADE 备选路线。
- 移除 STEP 导出要求，不为满足交换格式而引入其他内核或转换服务。
- C++ 保留 SMLib、Document、事务、建模与拓扑，并抽取核心 Commander。
- 对外服务、MCP、CLI、桌面 GUI、WebUI 使用 Rust；Python 仅作为内部测试和开发工具。
- Agent、桌面和 WebUI 为平级客户端，默认连接共享 CAD 服务；保留显式独立模式。
- GUI 采用 egui / eframe，三维视口使用 wgpu；桌面与 Web 尽量复用界面与客户端代码。
- P1–P4 已实现 Commander、Rust 共享服务/CLI/MCP 和 CPU 场景渲染；Python 原型仅供内部回归。
  egui GUI/WebUI、GPU Picking 等仍为后续目标，不把目标误写为已实现能力。
- 所有建模修改必须失败回滚；保留独立 Document、Undo/Redo 和版本管理目标。
- 保留 Shaded、Face ID、Edge ID，以及 VLM 视觉定位 + GPU Picking + 几何查询目标。
- 不向 Agent 暴露内核指针，保持公共建模接口与 SMLib 实现解耦。

## 最小建模工作流

1. Agent 创建 Box，获取服务返回的模型状态。
2. 请求普通光照截图。
3. 根据截图与拾取工具选择 Edge。
4. 执行 Fillet，并由内核验证模型有效性。
5. 获取结果截图。

当前原型通过 CPU 像素拾取运行该流程，GPU Picking、真实 VLM 集成、持久命名与磁盘版本恢复
仍是后续工作。原生 SMB 导出作为已有附加功能保留，不增加 STEP 格式验收要求。

回滚采用新的 Git 提交保留历史；此前 OCCT / STEP 阶段的实现、依赖配置、测试和指南已移出当前版本。

2026-10-18：补充共享服务、Rust 对外组件和 egui 决策。详细边界见 [架构文档](architecture.md)，执行顺序见 [实施计划](implementation-plan.md)。
