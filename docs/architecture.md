# 共享服务、Commander 与 Rust 客户端架构

日期：2026-10-18。状态：核心方向已确认；P1–P7 Commander、共享服务、Rust CLI/MCP、GPU/CPU 场景渲染与 egui 桌面/Web 已实现，其余目标尚待迁移。
本文取代首轮 MVP 的目标架构描述；现有实现情况见第 2 节。
实施顺序与验收门槛见 [实施计划](implementation-plan.md)。

## 1. 已确认的决策

| 决策 | 约束 |
|---|---|
| Agent-Agnostic、Headless-First | 不开发专用 LLM，不绑定特定 Agent；服务无 GUI 也能运行 |
| 默认共享 CAD 服务 | 桌面 GUI、WebUI、MCP、CLI 默认连接同一服务，可显式选择同一会话和文档 |
| 接入层平级 | Agent 不通过 GUI 才能建模，GUI 不通过 Agent 才能操作；入口共享同一命令语义 |
| C++ 核心 Commander | 保留 C++17、SMLib、文档、事务、建模和拓扑；抽取统一应用门面 |
| Rust 对外组件 | 服务入口、客户端 SDK、MCP、CLI、桌面 GUI、WebUI 使用 Rust |
| egui GUI | egui 负责界面；eframe 负责桌面/Web 宿主；wgpu 用于三维视口和后续 GPU Picking |
| Python 仅供内部工具 | 可用于测试、数据生成和开发脚本；最终对外运行及发布包不依赖 Python |
| 只使用 SMLib | 不恢复 OCCT 备选路线；不恢复 STEP 导出要求 |
| 保留建模正确性要求 | 不暴露内核指针，失败回滚，Undo/Redo，版本前置条件，过期引用拒绝 |

Rust 迁移的主要收益是统一入口实现、复用类型与 SDK、减少维护边界；不据此承诺建模提速。
几何性能仍取决于 SMLib、数据交换和渲染。默认共享不等于默认多人协同编辑，也不等于自动共享所有文档。

## 2. 当前实现基线

迁移前基线为 4062637 所在的 SMLib MVP 阶段；P1–P7 后的现状如下：

- C++ `cad-commander` 管理文档、注册/校验/分发命令并组织响应；`cad-server` 仅负责 JSONL 收发。
- `cad-document` 保存内存快照与 Undo/Redo；建模采用 copy / validate / commit，Commander 暂存文档并在响应构造成功后发布。
- Rust MCP/CLI 通过 cad-access / cad-client 默认连接共享服务；原 Python 实现已移至 tests/legacy，旧启动路径仅提示迁移。
- Rust cad-render 由共享服务统一调用，帧缓存跨 CLI/MCP 共享；CPU 场景截图无需窗口，新增 egui 桌面，原生宿主/异步线程与共享 UI 分离。
- 已有 Box、单边 Fillet、拓扑查询、截图、原生 SMB 导出；Boolean 在内核探针层验证。
- 已有 Rust cad-protocol / cad-client / cad-service，默认共享服务、隔离会话、显式 attach、文档快照及带游标的事件拉取；已有 egui 桌面/WebUI；已有服务端 wgpu 光照/深度/Face ID/Edge ID 与共享帧拾取；尚无持久 Naming 或磁盘恢复。

P1 增加 commands.list 与错误 kind；P2 将描述移到中性 contract.json，加入异步 Rust 客户端、版本与重试契约。
通过双端共用样例、类型覆盖和真实 worker 校核，而非代码生成。P3 加入本机 loopback 共享服务、OS 文件锁、实例发现、内存事件与实际重连核对；P4 已替换 Rust MCP/CLI，
将截图/拾取移到服务，并提供原生发布包和迁移说明。
Python 仅作为内部回归/构建工具；旧产品路径只保留拒绝启动的迁移提示，不进入发布包。

## 3. 参考项目及采用原则

源码调查快照：

- [PhotoCraft](https://github.com/storytold/photocraft/tree/8d2e615ab93d424c4ec25ab15b4edb5368d1f9a6)：
  `CommandSpec`、`Session`、命令日志、快照后台任务、egui 桌面/Web 复用。
- [CADCraft](https://github.com/storytold/cadcraft/tree/b23d362f6af1dd77e319728f1d85882b098626fe)：
  `CommandSpec`、交互命令状态机、预览、MCP Headless/Remote 两种访问方式、可替换 UI。

采用：统一命令、薄客户端、显式交互状态、共享业务语义、按依赖检查分层。
适配：我们的核心仍是 C++/SMLib；Web 首版连接服务端精确建模核心，不要求 SMLib 编译为 WASM。
不照搬：像素 tile 写时复制、绘图实体数据库、纯 Rust 内核路线，或参考项目的全部功能规模。
参考项目的声明与实现可能有差异，应以固定提交的相关源码为依据，不将其性能或稳定性声明视作本项目保证。

## 4. 逻辑结构与语言边界

```text
Rust MCP / CLI       Rust egui 桌面       Rust egui WebUI（WASM）
       │                    │                       │
       └──────── 统一协议 / Rust 客户端 SDK ─────────┘
                            │
                    Rust 共享服务入口
              连接 / 会话路由 / 请求跟踪 / 事件分发
                            │ 内部进程协议
                    C++ CAD core worker
                       C++ Commander
              命令发现 / 查询 / 执行 / 事务 / 调度
                            │
              Document / Modeling / Topology
                            │
                      SMLib 适配层
```

这是依赖和访问关系，不要求每个模块单独部署。首版可以由一个 Rust 服务管理一个 C++ worker，
worker 内串行处理内核调用，管理多个文档；后续再按隔离需求增加 worker。
SMLib context 的约束不因引入 Rust 异步运行时而改变，不能直接并发调用同一个内核 context。

### 4.1 Rust 对外服务

负责连接管理、服务发现/启动、会话到 worker 的路由、请求 ID、响应和事件传递、客户端断连处理。
业务修改的最终校验、权威文档和 Undo/Redo 均留在 C++ 核心。
Rust 可以提前校验输入结构，但不能另写一套 Fillet 参数规则、拓扑身份或事务实现。

### 4.2 C++ Commander

Commander 是统一应用门面，内部拆分注册表、处理器、任务调度和会话服务，避免成为单个巨型类。
它不包含网络协议收发、egui 控件、MCP 格式、文件对话框或具体几何算法。

| 能力 | 职责 |
|---|---|
| 命令发现 | 稳定命令 ID、输入/输出 schema、单位、可用条件和禁用原因 |
| 校验 | 文档存在、revision、实体引用、参数范围、业务前置条件 |
| 修改执行 | 在候选副本上操作，内核验证后提交，失败保持模型与历史不变 |
| 查询 | 返回文档摘要、拓扑与显示数据；查询不创建 Undo 步骤 |
| 任务 | 跟踪耗时操作的状态与结果，提供可支持的取消语义 |
| 事件 | 提交后产生带文档 ID/revision 的变化事件，由 Rust 服务转发 |

几何算法继续由 `cad-modeling` / `cad-kernel` 实现。文档历史继续由 `cad-document` 管理。

### 4.3 egui、视口与平台宿主

桌面和 Web 共用 Rust UI、交互控制器与视口代码；平台宿主处理窗口、浏览器、文件选择和传输差异。
egui 负责控件与布局，不自动提供精确 CAD 三维渲染或拓扑拾取。
视口消费中性网格、边折线、实体引用、相机等数据，不持有 SMLib 对象。
wgpu 后端负责 Shaded / Face ID / Edge ID、深度和拾取；服务端离屏渲染不依赖打开 GUI 窗口。
具体渲染进程位置与设备管理方案在进入视口阶段前验证，不在文档阶段固定额外进程。

## 5. 共享会话与状态归属

### 5.1 两种运行模式

- 默认连接共享服务；服务不存在时启动一次并等待就绪，多个客户端同时启动不能创建重复实例。
- GUI、WebUI、MCP、CLI 可以列出/选择会话和文档，显式连接相同目标。
- GUI 关闭只断开客户端，不结束仍被其他客户端使用的服务；服务关闭采用显式管理操作。
- 保留显式独立会话/独立实例用于测试和批处理，不影响默认共享文档。
- 客户端断连不等于取消已接受的修改；不得因断连直接销毁文档。

首版以同一用户、本机服务为基线；Web 接入通过 Rust 服务提供，不能把本机共享等同于公网多租户授权。

### 5.2 权威状态与客户端缓存

| 状态 | 管理方式 |
|---|---|
| 文档与几何 | C++ 核心唯一权威；Rust 服务记录路由，客户端只缓存读模型 |
| Document revision / Undo / Redo | 核心管理，修改成功后递增；失败不改变，Undo/Redo 也产生新 revision |
| 相机、缩放、显示模式 | 按视口管理；需要共享时显式指定 viewport，而非所有入口共用全局相机 |
| 选择、悬停、输入草稿 | 默认按客户端/交互会话隔离；共享选择需显式操作 |
| 面板、窗口、布局 | 客户端 UI 状态，不污染文档历史 |
| 任务状态 | 核心报告执行结果，服务维护对外请求与任务关联 |

模型修改请求必须显式携带文档 ID、期望 revision 和所需实体引用。
不能使用另一个客户端的隐式“当前选中边”执行 Agent 请求。
几何 revision 与相机/界面变化分开，旋转视图不使几何引用失效。

### 5.3 多入口修改与事件

首版通过串行执行和乐观版本检查处理冲突，不实现自动合并。
两个入口基于同一 revision 修改时，只接受符合当前版本的操作，另一个收到明确冲突并重新查询。
事件最少表达文档创建、修改、Undo/Redo、任务完成/失败，以及所属文档和 revision。
客户端发现事件缺失、重连或服务重启后重新获取快照，不能靠事件猜测完整模型。
请求跟踪与重试规则须防止不确定结果下重复修改；具体机制在协议阶段确定。

## 6. 统一命令与交互

协议区分只读查询和文档修改，共享一个可发现的命令目录。
协议 schema 应有一个中性来源，生成或校核 Rust/C++ 的边界类型，避免手工维护多份定义。
菜单、快捷键与本地化可保留在 UI 目录；核心不绑定 egui 风格的 UI 元数据。
MCP 工具描述、CLI 帮助和简单参数面板尽量从相同命令描述派生。

Agent 可以一次提交完整参数；GUI 可通过选择、参数编辑、预览、确认/取消来组装同一业务命令。
预览应绑定基础 revision，取消不改变正式文档，确认只提交一个历史步骤。
高频鼠标移动、相机操作不必都经过远程建模调用；昂贵预览应合并、节流并丢弃过期结果。
浏览器和桌面 UI 使用异步客户端，不在 egui 绘制循环中同步等待内核操作。

## 7. 渲染与拾取一致性

截图、网格和 ID 映射必须标识 document/revision、viewport/camera、尺寸与 render ID。
普通截图供视觉定位，ID picking 返回实体引用，几何查询用于消歧。
拾取结果不能越过 revision 检查；不确定或多个候选不能静默选择错误实体。
服务端截图与本地视口逐步共享场景数据和渲染实现，不能长期各自定义一套拓扑映射。

P4 已以 Rust CPU 路径接替截图/拾取，统一从共享服务获取；Python 版本仅位于 tests/legacy 作为内部回归。
服务在同一 worker 锁内获取 mesh/edges 和版本，CPU 计算使用 spawn_blocking；帧携带完整上下文，pick 校核核心版本。
后续 GPU/窗口视口仍需复用同一场景/引用语义，不能将当前 CPU 近似拾取宣称为 GPU Picking。

## 8. 模块演进建议

名称是实施建议，目录在相关阶段收敛，不要求一次性创建全部模块。

| 模块 | 语言 | 目标 |
|---|---|---|
| cad-commander | C++ | 从现有 main.cpp 抽出的应用门面与处理器 |
| cad-core-worker | C++ | 内部 worker 宿主，持有 Commander，处理内部协议 |
| cad-document / cad-modeling / cad-topology / cad-kernel | C++ | 保留并扩展现有核心职责 |
| cad-protocol | 中性 schema + Rust 类型 | 命令、查询、事件、错误、版本与能力协商 |
| cad-client | Rust | 桌面、Web、CLI、MCP 共用的异步接口和客户端缓存 |
| cad-service | Rust | 对外共享服务、worker 管理、连接与事件 |
| cad-cli / cad-mcp | Rust | 薄适配器，替换 Python 产品入口 |
| cad-ui-egui / cad-viewport | Rust | 共用界面、交互控制器、三维视口和拾取 |
| 桌面 / Web app | Rust | eframe 原生宿主 / WASM 宿主 |
| 内部脚本、回归测试 | Python 可选 | 开发工具；不成为运行时依赖 |

## 9. 尚待技术验证的细节

已确认的是语言、职责与共享方向；以下实现选择仍需按计划验证：

- P2 内部采用 JSONL pipes；P3 共享连接已采用 loopback TCP，Unix socket / named pipe 按后续需求适配。
- 浏览器 HTTP/WebSocket 接口、会话访问控制，以及与本地协议的适配方式。
- P2 已确定 JSON 中性契约、响应内事件、有限缓存与高水位；P3 已实现带游标的增量事件拉取、重连和多客户端路由，暂不提供推送订阅。
- 网格二进制传输、增量更新和缓存策略，以实测决定，不先优化全部消息。
- 视口与离屏渲染的复用方式、GPU 兼容性和软件渲染 fallback。
- 原生安装包与 CMake/Cargo 构建整合、跨平台服务生命周期实现。

未列入本次架构迁移的承诺：SMLib WASM、Rust 重写几何核心、Rust/C++ FFI、多人实时协同、
新增交换格式或备用内核。持久 Naming、磁盘版本恢复仍是原有后续目标，另设验收，不因共享服务而宣称完成。


## P5 实现补充

cad-ui-egui 只管理 UI/读模型/动作，cad-gui 负责原生宿主和 cad-client 的后台连接。
核心 render.scene 原子返回 mesh/edges/状态/历史可用性；model.preview 在临时 Document 副本上复用建模处理器，
不通过修改正式文档再 Undo 来预览。候选使用不同引用命名空间，确认后重新读取正式场景。
相机/选择/悬停/草稿只在窗口内；UI 结果以文档 revision、ticket、视图 epoch 匹配，拒绝迟到或过期结果。
详情见 [P5 桌面与线程设计](desktop-p5.md)。

## P6 实现补充

cad-ui-egui::Controller<T: Transport> 是桌面/Web 共用的异步命令策略；原生 Bridge 和 WASM 宿主只提供调度及传输。
Web 的 Rust HTTP gateway 将浏览器授权限定到指定 service/worker/session，各页面独立 client ID；
不暴露内部服务 token、生命周期接口或任意服务器文件路径。默认采用同源 POST + 事件轮询，避免为首版引入第二套 WebSocket 事件语义。
浏览器 CPU raster 在模块 Web Worker 内执行，主线程应用带 epoch/revision 的帧；WebGL2 只负责 egui/纹理显示。
Web 与 CAD daemon 使用独立持久进程；网关 attach-only，网页重部署不拥有共享文档寿命。
授权、下载、安全边界、构建和发布见 [P6 说明](web-p6.md)。

## P7 实现补充

服务端 cad-render 的 wgpu compute 两遍完成光照面覆盖/浮点深度和可见边 ID；桌面/Web/Agent 共享 Frame，
界面不再各自重算用于精确选择的 ID 图。Web Worker 为异步视口传输宿主，浏览器不承担 SMLib 或另一套 GPU 几何状态。
保留 CPU fallback 与真实适配器报告。渲染入口可作为附着现有 worker 的独立 relay 升级，原文档/历史/请求身份不变。
缓存、压缩协议、版本/视口验证和限制见 [P7 说明](viewport-p7.md)。


### 2026-10-19：Web 连续交互视口修正

P7 的 Web 主视口实际展示服务端逐帧返回的位图；相机改变立即隐藏过期帧，造成拖动时白屏，且严格 epoch 校验使连续拖动中的返回帧被丢弃。Web 现改用 `cad-ui-egui::local_view` 的 wgpu raster 管线直接绘制已获取的同一份 `Scene.mesh / edges`，采用深度缓冲和可见边；网格上传按场景身份缓存，相机输入只更新 48 字节 uniform。浏览器优先 WebGPU，缺少适配器时通过 wgpu 回退 WebGL2。没有 GPU 读回或网络往返参与相机绘制。

相机静止 150ms 后再请求服务端 Frame，用于原有 ID 拾取、截图和确认前校验。服务端仍提供 headless GPU/CPU 渲染；绘制不依赖 Frame 是否到达，过期 ID 帧仍不可用于选择或确认。修改仅作用于 Web 启用的本地 GPU 绘制，桌面当前默认显示路径继续保留。样式、相机投影及拓扑 reference 共用协议；客户端三角化显示并非精确 B-Rep 求交。


### 2026-10-19：共用的平板布局与触屏视图

Workspace 统一使用文档 tabs、顶部矢量工具带、左侧可收起 PropertyManager 和透明叠加模型树。树的操作记录来自现有事件查询，不能视为可回编的参数化特征树。相机支持锚点缩放、双指平移及多指结束保护；按比例生成有限尺寸的拾取图。桌面与 Web 共用 local_view GPU raster，服务端仍负责 B-Rep、文档、版本和 headless ID / capture。只读 UI 几何桥为自动化返回实际控件位置，既不承担 CAD 状态权威，也不暴露内部 endpoint/token。详见 `docs/tablet-ui.md`。

2026-10-19：新增同级 `cad-agent` 宿主适配与版本化引用上下文。共享 egui 对话面板产生 Turn；Web/native 宿主独立异步调用本机 Agent，仅返回 typed Proposal，经权限/真实场景/revision/显式引用校验后，由 UI 进入既有 Commander 预览/确认链路。内核、MCP、CLI 不依赖 Pi 或任何模型；完整边界见 `agent-workspace.md`。
