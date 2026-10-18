# 共享服务与 Rust/egui 改造计划

日期：2026-10-18。状态：P1–P6 已实现并验收；下一阶段 P7。
依据：[架构决策](architecture.md) 与 [项目范围](scope.md)。
阶段顺序按依赖排列，不作未经验证的工期承诺。每阶段通过验收后提交本地 Git commit 并报告，
不擅自推送。迁移中保持现有 SMLib 能力可运行，按实际代码更新状态与使用说明。

## 0. 基线与执行原则

迁移前基线：SMLib、C++ JSONL worker、Python MCP/CLI、Python CPU 预览。
当前 P6 已具有 Rust 共享服务、CLI/MCP、CPU 场景渲染与 egui 桌面/Web 工作区。
P1 的验收结果见下节；不把既有 Python 原型计作 Rust 阶段完成。

- 优先证明同一命令的结果、回滚和引用语义，再扩大界面功能。
- 关键边界先写能失败的契约测试，再实现；不为文档或简单重命名单独堆测试。
- 不修改第三方内核；不迁移核心为 Rust；不加入 OCCT 或 STEP 导出。
- 对外 Python 退出包含 MCP、CLI、连接逻辑、截图和拾取，而不只替换启动脚本。
- 内部 Python 测试可保留；不将 Python 作为最终应用运行或安装的前置条件。
- 新入口必须通过 Commander，不能再开绕过事务的内核调用路径。

## P1：抽出 C++ Commander

目标：先建立应用核心边界，不改变既有传输和行为。

- [x] 从 `modules/cad-server/main.cpp` 抽取文档管理、命令分发、校验和响应结果。
- [x] 让 main 主要处理 JSONL 收发，Commander 可在无窗口、无网络环境独立测试。
- [x] 保留当前命令 ID、单位、revision、实体引用与 copy / validate / commit 语义。
- [x] 建立基础命令描述：命令 ID、输入/输出、只读/修改、前置条件与错误分类。

验收：

- 现有内核探针、事务测试、MCP/CLI/预览回归全部通过。
- 同一 Commander 实例可执行 Box、查询、Fillet、Undo/Redo；没有服务循环才能调用的业务逻辑。
- 失败不改变 body/revision/history/redo；未知命令和过期引用有明确错误。
- 本阶段不宣称 Python 已退出，也不建立第二套命令实现。

交付：Commander 库、薄 worker、边界测试；同步记录新目录与当前协议兼容性。

P1 完成记录（2026-10-18）：

- 新增 cad-commander 库；cad-server 仅保留 JSONL 收发、解析与传输错误。
- 增加 commands.list 与命令描述；既有错误 code 保留，新增 kind 分类。
- 对暂存 Document 完成操作和响应构造后再发布，保持失败回滚与 redo。
- 测试先跑出预期失败，再完成实现；CTest 4/4 通过，其中 Commander 含 8 个直接契约场景。
- 原有 CLI/MCP/CPU 预览通过，示例成功导出有效 Fillet 的 SMB 文件。
- Python 产品入口仍是过渡实现，尚无 Rust workspace、共享服务或 GUI。

## P2：统一协议与 Rust 客户端基础

依赖 P1。目标：Rust 入口有共同类型与访问方式。

- [x] 创建最小 Rust workspace 和协议/客户端模块。
- [x] 确定一个中性 schema 来源，明确 Rust/C++ 类型生成或一致性校核方法。
- [x] 定义命令、查询、结果、结构化错误、事件、能力与协议版本。
- [x] 定义 document/session/client/request ID、expected revision、worker/service 实例标识。
- [x] 定义请求被接受后断连的语义、未知执行结果的查询/去重方式，禁止盲目重发修改。
- [x] 保持小型控制消息易调试；将大型网格传输作为独立接口设计，暂不强制全二进制。

验收：

- Rust 客户端调用同一个 C++ Commander 完成 Box/Fillet/查询。
- 双端序列化、schema 和错误码有契约测试；非法输入、非有限数值、版本不匹配明确拒绝。
- Rust 不实现独立几何业务规则；核心仍能独立验证所有输入。
- 文档 revision 不因查询或视图更新改变。

交付：协议基线、Rust 客户端、跨语言契约测试；记录本地与浏览器传输的具体选择及理由。

P2 完成记录：

- 增加 Rust workspace、cad-protocol / cad-client 和固定 toolchain/Cargo.lock。
- protocol/contract.json 为唯一中性来源；C++/Rust 同读，使用一致性校核而非代码生成器。
- 已定义并验证版本、标识、命令/结果/错误、响应内事件与有限缓存/高水位去重语义。
- Rust 异步客户端直接完成真实 SMLib Box/Fillet/查询/Undo/Redo，无 Python 运行依赖。
- 32 个共用契约样例；Rust 另验证有限 f64 位级往返和断连/超时/取消/关联错误不自动重发。
- 旧版 Python 产品入口兼容；仍无默认共享服务、Rust MCP/CLI 产品入口或 GUI。

## P3：默认共享 Rust 服务

依赖 P2。目标：多入口连接同一份权威文档。

- [x] 引入 Rust 服务入口，管理 C++ worker 生命周期与会话路由。
- [x] 实现发现、自动启动、就绪握手、实例识别与显式关闭。
- [x] 防止两个客户端同时启动产生重复默认服务；处理失效端点/锁的恢复。
- [x] 提供会话/文档列举与显式 attach；保留独立模式用于批处理和测试。
- [x] 加入提交事件与客户端快照刷新，定义事件序列、重连和漏事件恢复。
- [x] 同一 worker 串行执行内核调用；服务的异步任务不并发操纵 SMLib context。
- [x] 区分客户端断开、作业取消、worker 崩溃和服务关闭；崩溃不能伪报事务成功。

验收：

- 两个独立客户端连接默认服务：A 创建文档，B 可查询同一 ID 和几何状态。
- A 成功修改后 B 收到变化并刷新；两个旧版本请求不会同时提交。
- 一个客户端退出后，另一个仍能操作原文档；多个启动请求只得到一个默认实例。
- 断连重连后重新同步；不确定结果下重试不会导致重复创建/修改。
- 独立模式不污染默认会话；此处只承诺内存共享，不承诺磁盘崩溃恢复。

交付：可用共享服务、服务/worker 状态诊断、多客户端集成测试。

P3 完成记录（2026-10-18）：

- Rust cad-service + SharedClient 已实现；本机 loopback JSONL，OS 生命周期文件锁和原子就绪发现。
- 默认会话共享、幂等隔离会话、显式 attach/关闭、快照、带游标的增量事件拉取和漏事件恢复已验证。
- 同 worker 串行；断连/取消不撤销已接受请求；保留原请求并通过同实例状态查询/去重核对。
- 四组真实 daemon 集成测试覆盖启动竞争、revision 冲突、断连、取消、worker/daemon 崩溃和陈旧端点。
- 两次独立示例进程验证共享服务存活和文档可见性。只承诺内存共享；事件采用 pull，无 HTTP/WS/GUI。
- 操作说明与边界见 [服务文档](../crates/cad-service/README.md)，结果见 [验证记录](validation.md)。

## P4：Rust MCP、CLI 与无 Python 产品路径

依赖 P3。目标：替换 Python 产品入口，并补齐其隐藏的渲染职责。

- [x] Rust CLI 和 MCP 共用 cad-client，默认 attach，共享命令描述与错误处理。
- [x] 保留可验证的协议兼容路径或给出明确迁移说明，不让 Agent 配置无提示失效。
- [x] MCP 实现生命周期、工具发现、调用、结构化错误和图像返回。
- [x] 提供最小 Rust 截图/拾取路径，共用场景与引用映射；首版允许 CPU 实现。
- [x] 让所有入口请求同一渲染能力，移出 Python 客户端里的产品逻辑。
- [x] 替换使用指南、启动配置和发布入口；Python 原型退出产品目录或移为内部测试工具。
- [x] 区分 CAD 场景截图和 GUI 截图：无界面场景截图必须可用，GUI 截图可待后续阶段。

验收：

- Rust CLI 和 MCP 分别连接同一个服务，完成创建、查询、Fillet、Undo/Redo、截图及拾取。
- 在运行环境没有 Python 的情况下，预构建发布产物完成上述流程和原生 SMB 导出。
- 与原型的解析体积、有效性、错误回滚、引用失效结果一致。
- 首版渲染允许有 CPU/GPU 性能差异，不放宽实体映射和版本正确性要求。
- stdout 只包含相应协议/命令输出，日志不破坏 MCP/机器调用。

交付：Rust 服务/MCP/CLI 产品路径、Python 退出清单、运行与迁移指南。

P4 完成记录（2026-10-18）：

- Rust cad-cli / cad-mcp 共用 cad-access / cad-client，默认共享服务；旧工具名、JSONL 命令及核心语义保留。
- neutral contract 的 commands + access_commands 驱动产品目录、MCP inputSchema 和参数校验。
- Rust cad-render 在服务端生成 shaded / face-id / edge-id PNG；共享帧缓存、版本检查、背景/歧义和淘汰处理。
- MCP 生命周期、版本协商、协议/工具错误、structuredContent + text + PNG image 已验证；不声明 HTTP/tasks。
- Python 原型已移到 tests/legacy，旧路径仅迁移提示。release 包仅含四个原生程序、文档与第三方声明。
- 真实 CLI/MCP 多进程闭环及无 Python PATH 的发布包运行验收，详见 [验证记录](validation.md)。
- [迁移指南](migration-p4.md) 明确共享生命周期、images base64 / files 路径变化；GUI 场景另待 P5。

## P5：最小 egui 桌面 GUI 与异步交互

依赖 P4。目标：GUI 成为共享服务的普通客户端。

- [x] 使用 egui + eframe 建立桌面宿主，共用 Rust client 与读模型。
- [x] 增加视口、文档/模型树、Box/Fillet 参数、状态/错误、Undo/Redo。
- [x] 参数面板和选择生成既有业务命令，UI 不直接调用 SMLib。
- [x] 按客户端区分相机、选择、悬停与输入草稿，显式指定操作文档。
- [x] 实现异步等待与任务状态；耗时内核调用不阻塞界面绘制。
- [x] 实现最小预览/确认/取消；确认一次编辑只产生一个历史步骤，取消不改正式文档。

验收：

- GUI 与 MCP/CLI 同时附着：任一入口修改，其他入口可见同一结果。
- GUI 关闭不终止 Agent 使用的服务；重开后能重新连接原有会话。
- 旋转视图不改变文档 revision；Agent 不会意外消费 GUI 的隐式选择。
- 预览过期、操作失败、服务不可达均有明确反馈。
- 查看真实截图/录制的交互结果；不能只靠 UI 编译通过验收。

交付：最小桌面客户端和共享会话演示，不以完整 CAD 菜单数量为目标。

P5 完成记录（2026-10-18）：

- cad-ui-egui 复用读模型/控制器，cad-gui 用 eframe/wgpu 建立原生宿主，直接使用 cad-client。
- 核心新增原子 render.scene 与只读 model.preview；候选副本计算、独立引用命名空间，确认只提交一次原命令。
- GUI 按窗口隔离相机/选择/悬停/草稿；后台服务线程与 CPU 渲染线程、任务 ticket/epoch 拒绝迟到结果。
- 真实 Xvfb 窗口与独立 CLI/MCP 完成 Box/Fillet、预览取消、Undo/Redo、相机不改版本、外部修改使预览过期、关闭/重开和断连反馈。
- 已查看真实截图；当前 UI 使用 wgpu 显示 CPU raster 纹理，GPU 几何/拾取留待 P7。
- [桌面说明](desktop-p5.md)、[验证记录](validation.md) 与 artifacts/p5-gui 的 PNG/SMB/JSON 记录。

## P6：复用 egui 的 WebUI

依赖 P5，浏览器协议适配可在 P2/P3 预先验证。目标：复用 UI，连接服务端 C++ 核心。

- [x] 将 UI/视口代码与原生窗口、文件和进程能力隔离，建立 WASM 宿主。
- [x] 完成浏览器到 Rust 服务的异步传输与状态同步；不要求 SMLib 运行在 WASM 中。
- [x] 补齐会话授权、来源检查、断线反馈与重新同步；不直接暴露内部 worker。
- [x] 采用浏览器文件/下载接口，避免把任意服务端路径直接作为网页文件选择。
- [x] 遵循用户的 tailnet-publish 指令：实施时读取指定 SKILL.md，处理 base path、持久服务和门户收录。
- [x] 默认发布目标为 `https://example.ts.net/apps/<slug>/`，slug 为 miocad，已实际发布并收录门户。

验收：

- 桌面与 Web 共用 UI/命令客户端，差异收敛在平台宿主及传输适配。
- 浏览器、桌面、Agent 能操作被授权的同一个文档并收到一致事件。
- 断线后恢复快照；未提交的 UI 草稿不会被误认为已成功建模。
- 无 Python 运行时、无需浏览器侧 SMLib、无需打开桌面窗口。
- 按发布技能验证 HTTPS、base path 静态资源/连接、服务重启存活与门户入口。

交付：WebUI、浏览器适配和实际发布验证记录。

P6 完成记录（2026-10-18）：

- 新增 cad-web Rust HTTP gateway + egui WASM 宿主，桌面/Web 共用 cad-ui-egui Controller<T: Transport>。
- 默认会话授权、精确 Origin、自定义请求头、HttpOnly/SameSite/Secure cookie、独立页面身份、配额与过期/注销；内部 endpoint token 不进入浏览器。
- 同源异步 HTTP + 450 ms 事件轮询，重连快照与 request.status 核对；真实丢失建模回执没有重复提交。
- Web Worker 复用 CPU raster/picking，基于 epoch/revision 拒绝旧帧；SMB 通过私有临时目录和浏览器 Blob 下载，无任意服务端路径。
- 11/11 CTest（包含桌面及真实 Chromium）、26 个 Rust 测试、native/WASM Clippy 和 fmt 均通过。
- HTTPS、JS/WASM/Worker/API、嵌套页面、cookie Path、门户在线检查通过；网关重启后原 service/文档/revision 保留。
- 门户大型 base64 校验栈溢出，检查日志后改用技能允许的独立 systemd + proxy；未改门户/内核代码。
- [Web 说明](web-p6.md)、[验证记录](validation.md) 和 artifacts/p6-web 保存截图/JSON；无 Python 产品运行时。

## P7：共享视口、GPU Picking 与整体验收

依赖 P5/P6；wgpu 技术验证可提前，但不改变交付顺序。目标：完成统一视觉选择路径。

- [ ] 完善 wgpu Shaded / Face ID / Edge ID / depth pass 与实体引用映射。
- [ ] 复用桌面/Web 场景数据，定义服务端无窗口离屏渲染的复用和 fallback 方式。
- [ ] 建立基于 revision / camera / viewport / render ID 的截图与拾取一致性检查。
- [ ] 处理遮挡、边容差、多个候选、旧帧、异步结果乱序和 GPU 不可用情况。
- [ ] 补充网格缓存/更新与代表性性能测量；按证据优化传输和重绘。
- [ ] 在实际通用 Agent host 验证 MCP，记录客户端与版本；真实视觉选择与确定性测试分开报告。

验收：

- 桌面/Web/Agent 在同一指定视口上获得一致的实体选择，过期图像不能用于修改。
- Headless 截图不要求打开 GUI；GPU 路径与 fallback 能力如实报告。
- 创建 Box → 截图定位 → 选 Edge → Fillet → 内核校验 → 结果截图完整通过。
- 对外服务/CLI/MCP/UI 路径无 Python；内部 Python 回归测试可以继续运行。
- 不把本阶段通过等同于持久 Naming、磁盘历史恢复或协同编辑已经完成。

交付：共享服务与多入口闭环、GPU/回退验证记录、性能基线和剩余问题清单。

## 后续独立工作

持久 Naming、磁盘快照/日志恢复、多 body/Boolean 文档命令、特征树及更完整跨平台分发，
在上述基础上分别立项。其设计边界应在协议中预留，但不把所有能力塞进第一次 Commander 抽取。

## 下一次执行的起点

下一阶段为 P6：复用现有 egui UI/视口与协议，增加浏览器传输、会话授权及 Web 宿主。
P5 原生/无界面路径继续独立运行；Web 应用发布须按门户规则执行，本阶段未创建或发布 Web 应用。
