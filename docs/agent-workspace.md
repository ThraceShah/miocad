# 自主执行的 CAD Agent 工作区

2026-10-19。默认自主执行，无用户审批步骤。用户发送目标后，宿主持续让 Agent 读取当前状态、选择操作、调用 C++ Commander、验证 B-Rep，再把实际执行结果反馈给 Agent，直至完成；失败会修正或回滚，用户可撤销整个任务。目前执行能力为 Box、单边定半径 Fillet。Agent、egui UI 和 WebUI 仍为同级接入层；不在内核内绑定模型，也不新增 Python 产品服务。

## 使用

1. 顶部 **Agent** 打开可调整高度的底部对话面板。浏览状态默认收起，适用于横屏和竖屏。
2. 在面板设置 **Edge / Face / Body**，点击视图中的目标，会生成 `@Edge 2`、`@Face 1`、`@Body 1` 等命名引用。重合边沿用原来的显式候选选择器。
3. 点击树中的 Box / Fillet 操作记录可生成 `@Box 1`、`@Fillet 1`；点击 Body 或展开 Faces / Edges 也能定位相应元素。引用集合可包含多个元素，同一元素不会重复加入。引用名称是可读标签，执行始终使用结构化 target。
4. 引用边以粗紫线、面/体以金色表面高亮；普通当前边选择仍用琥珀色，悬停仍用青色。树中对应条目高亮，折叠的拓扑组显示 `referenced` 提示。点击当前版本的历史消息引用可重新加入上下文；× 删除引用，Clear references 清空。
5. Web 用 **Attach files / images** 选择 UTF-8 文本文件、PNG、JPEG；桌面把文件从文件管理器拖入窗口。第一版不解析任意二进制 CAD/PDF 附件。每文件约2.25MB，最多8个，总计4MB；文件名及图片作为附件显示在当前上下文和已发送消息中。
6. 输入“把引用的 Edge 2 做半径 1 mm 的定半径倒圆”，发送后直接得到已完成的模型和结果说明；不需要点击预览或确认。也可发送“替换为12×10×8毫米长方体，然后自主选一条边做1毫米倒圆”，Agent 会在 Box 成功后读取新几何、使用新 revision 的边 ID 继续倒圆。Box 会替换单个实体，因此仍要求用户目标明确包含替换。
7. 对结果不满意，点击 **Undo Agent task** 撤销本次任务的全部操作。任务运行在宿主后台，页面刷新后会查询原任务状态，不重新启动建模；手动建模工具仍沿用自己的预览/确认交互。

多指缩放/平移仍在视图内本地执行。对话文本支持中文，随产品包含 Apache-2.0 的 Droid Sans Fallback 字体及声明。聊天/附件是当前客户端的临时内存状态，不作为文档持久化功能；模型仍与 MCP、CLI、其他 UI 默认共享。

## 引用边界

引用携带 `{name, document, revision, kind, target}`。Edge / Face 使用已有不透明拓扑引用，Body 使用单实体文档下的 `body`，Feature 使用服务事件 ID `event:<id>`。名字不用于解析内核对象，不暴露原始指针。

当前 FeatureManager 存储的是不可变操作记录，不是可回编参数的特征图，也没有历史特征到当前生成面的映射。引用 Feature 时高亮树中该记录，并以整个当前实体作为视图上下文；面板与 tooltip 明示这一点，不声称这些面全部属于该历史特征。截断后不可获取的记录会被拒绝。

开始任务时查询共享服务的真实场景和事件，核验原始引用。第一步优先约束到用户显式引用的边；后续步骤以刚查询到的当前几何、当前 revision 和边目录为依据。Agent 可自主选择新建 Box 上的边；不把旧边 ID 按编号重新绑定。每个写入附带 revision，拒绝并发覆盖，内核验证失败的单步不提交。已执行操作、当前状态及内核错误都会进入下一轮推理上下文，支持修正失败的参数并继续。

默认32个建模操作预算（宿主 `CAD_AGENT_MAX_STEPS=1..128` 可调），最多3次内核拒绝；每次推理180秒、128KiB输出上限。不是一次长 HTTP 请求等待用户点按钮，而是后台任务 + 状态查询。已知失败会逆序 Undo 自己的操作；若其他客户端已有后续编辑，停止并报告 partial，避免撤销别人的修改。整体任务由多个内核事务构成，尚无跨客户端隔离的单个大事务。

成功完成后会清理已消费的活动引用，历史消息保留旧版本引用；过期引用不高亮新拓扑。`Undo Agent task` 仅在该任务结果仍为当前文档版本时启用，后端同样校验，防止跳过 UI 撤销后续编辑。重复 start / undo 不重复写入。

任务日志位于运行目录 `agent-tasks/`，包含任务状态、专用客户端 ID、写前请求和操作回执，不记录附件、原始聊天或凭证。不确定写入先查原 request.status，禁止重放；无法查明则报告 outcome_unknown。宿主重启会中断运行中的任务，保留日志并禁止自动继续；当前 Web 授权 grant 随网关重启失效，此时可检查当前文档/运行目录日志及普通 Undo 历史，尚未实现跨授权重建的自动任务接续。正常页面刷新保持原授权，可自动取回后台任务结果。

## Agent 适配

新增 `cad-agent` Rust crate：跨平台引用/附件/Turn/Reply 数据结构、校验，以及原生宿主薄适配。宿主从真实场景补充毫米单位、三角网格包围盒、选中边的长度与离散曲线作为上下文；网格包围盒不宣称是精确 B-Rep 查询。

本环境 Web 发布选择已安装 **Pi**，复用其账户和默认模型配置。不读取、复制或返回登录凭证。其他部署默认禁用，须由宿主显式选择：

```bash
CAD_AGENT_BACKEND=pi CAD_AGENT_PI="$(command -v pi)" target/release/cad-gui
# 可选 CAD_AGENT_MODEL=provider/model，未配置时使用 Pi 自己的默认设置。
```

Pi 在独立0700临时目录、无会话保存模式运行；关闭工具、MCP、扩展、Skills、模板和项目上下文加载，真实推理结果作为经校验的下一步操作，由宿主任务执行器调用 Commander。附件使用生成文件名传递，用户文本经 stdin 输入，不拼接 shell；结束/失败/超时清理临时文件。180秒宿主超时及128KiB输出限制。此为工具禁用和受限 CAD 操作协议，不是操作系统进程沙箱。

其他 Agent 可实现 **json-stdio** 适配器：宿主环境设置 `CAD_AGENT_BACKEND=json-stdio`，`CAD_AGENT_COMMAND_JSON` 为固定 argv JSON 数组。程序从 stdin 读取到 EOF 的单个 JSON 文档（`instructions`、`turn`、权威 `state` / `geometry`、`execution`），stdout 只返回一个 Reply JSON，退出0表示成功。浏览器不能设置命令行、可执行文件或任意服务器文件路径。

Reply：

```json
{"text":"执行 1 mm 倒圆。","proposal":{"kind":"fillet","reference":"本轮某个 Edge 的精确 target","radius":1}}
```

或 `proposal: {"kind":"box","size":[10,10,10]}`。内部字段 proposal 表示下一步操作，会直接执行。`proposal:null` 表示已完成或普通回答；无法完成目标时返回 `failed:true, proposal:null`，触发任务回滚。`execution.completed` 是已实际提交的操作，`execution.errors` 为内核反馈，`execution.edges` 是新 revision 的当前边目录。不要重复已完成操作，不要要求用户审批。拒绝未知操作、额外操作参数、伪造边和无效数值。变半径倒圆、凸台、拉伸、薄壳、布尔等当前未接入的操作必须明确说明，不通过关键词规则或定半径近似伪装实现。

Web `POST /api/agent` 接收 `{id,turn}` 并启动或返回已有后台任务；`POST /api/agent/status` 和 `/api/agent/undo` 接收 `{id}`，查询或启动整组撤销。沿用 Origin、Cookie、页面身份、固定 CAD service/session 授权，最多2个并行任务。任务不依赖浏览器继续在线，实际执行位于 Rust 宿主。MCP、CLI 核心建模协议保持不变。

## 测试

```bash
PATH="$HOME/.cargo/bin:$PATH" scripts/build-web.sh
PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace --locked
CAD_PLAYWRIGHT=/path/to/playwright ctest --test-dir build -R '^web_agent_references$' --output-on-failure
CAD_PLAYWRIGHT=/path/to/playwright CAD_AGENT_LIVE=1 node tests/web_agent.cjs
```

`desktop_agent_references` 另用隔离 Xvfb 驱动真实桌面窗口，验证原生 Bridge、树引用、自动提交和整组撤销，不出现 Agent 预览/确认按钮。

`web_agent_references` 使用真实 Rust 宿主和确定性子进程协议样本，覆盖四类引用、文本/图片传输、横竖屏布局、指定边自动倒圆、多步 Box→新边倒圆、整组 Undo、失败自动回滚、内核反馈修正、旧引用和伪造操作拒绝、丢失启动响应后的幂等查询、刷新期间后台完成、保护后续编辑以及跨源拒绝。`CAD_AGENT_LIVE=1` 单独调用已登录 Pi 做真实推理，不作为常规离线 CI 的模型依赖；所有建模均在独立临时 CAD 服务中。

## 长期运行的客户端接入

旧 C++ 协议端点最多保留128个客户端高水位。Rust service / render relay 在旧表满时，通过既有保留内核通道复用新的逻辑客户端，逻辑 client/request ID、写前预约、高水位、结果与 request.status 由运行目录下的 client-ledger 保持；过期结果只返回 outcome_unavailable，不重新执行。正常客户端继续走原通道。此兼容层可更新现有已注册渲染通道的 relay，保留旧文档拥有者、原 worker、文档及 Undo/Redo，不要求重启内存文档。

内联响应恢复逻辑客户端和请求 ID；旧拥有者汇总事件中的来源 client 目前仍可能显示保留通道，事件 ID、文档和 revision 不变。任务归属和撤销不依据该来源字段，而依据任务日志和文档版本。此为旧端点的兼容边界，不是通用 Persistent Naming 实现。
