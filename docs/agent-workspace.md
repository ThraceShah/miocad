# 带引用的 CAD Agent 工作区

2026-10-19。第一版以已有 Box、单边定半径 Fillet 为执行能力，保留 C++ Commander 的精确内核、事务、失败回滚和 Undo/Redo。Agent、egui UI 和 WebUI 仍为同级接入层；不在内核内绑定模型，也不新增 Python 产品服务。

## 使用

1. 顶部 **Agent** 打开可调整高度的底部对话面板。浏览状态默认收起，适用于横屏和竖屏。
2. 在面板设置 **Edge / Face / Body**，点击视图中的目标，会生成 `@Edge 2`、`@Face 1`、`@Body 1` 等命名引用。重合边沿用原来的显式候选选择器。
3. 点击树中的 Box / Fillet 操作记录可生成 `@Box 1`、`@Fillet 1`；点击 Body 或展开 Faces / Edges 也能定位相应元素。引用集合可包含多个元素，同一元素不会重复加入。引用名称是可读标签，执行始终使用结构化 target。
4. 引用边以粗紫线、面/体以金色表面高亮；普通当前边选择仍用琥珀色，悬停仍用青色。树中对应条目高亮，折叠的拓扑组显示 `referenced` 提示。点击当前版本的历史消息引用可重新加入上下文；× 删除引用，Clear references 清空。
5. Web 用 **Attach files / images** 选择 UTF-8 文本文件、PNG、JPEG；桌面把文件从文件管理器拖入窗口。第一版不解析任意二进制 CAD/PDF 附件。每文件约2.25MB，最多8个，总计4MB；文件名及图片作为附件显示在当前上下文和已发送消息中。
6. 输入“把引用的 Edge 2 做半径 1 mm 的定半径倒圆”。本机 Agent 返回说明和结构化建议，点 **Preview Agent proposal** 得到真实内核预览，再通过左侧 **Confirm / Cancel** 确认或取消。Box 会替换当前文档的单个实体，参数栏保留明确提示；Agent 被要求仅在用户明确要求替换时提出此操作。

多指缩放/平移仍在视图内本地执行。对话文本支持中文，随产品包含 Apache-2.0 的 Droid Sans Fallback 字体及声明。聊天/附件是当前客户端的临时内存状态，不作为文档持久化功能；模型仍与 MCP、CLI、其他 UI 默认共享。

## 引用边界

引用携带 `{name, document, revision, kind, target}`。Edge / Face 使用已有不透明拓扑引用，Body 使用单实体文档下的 `body`，Feature 使用服务事件 ID `event:<id>`。名字不用于解析内核对象，不暴露原始指针。

当前 FeatureManager 存储的是不可变操作记录，不是可回编参数的特征图，也没有历史特征到当前生成面的映射。引用 Feature 时高亮树中该记录，并以整个当前实体作为视图上下文；面板与 tooltip 明示这一点，不声称这些面全部属于该历史特征。截断后不可获取的记录会被拒绝。

每次请求前从共享 CAD 服务查询真实场景/事件，核验全部引用；Agent 返回后再检查 revision。UI 在预览前再次检查文档/revision、边引用必须来自本轮显式上下文、数值有效。模型变更后引用显示 `stale` 并停止高亮，旧建议禁用；不按新边编号自动重新绑定。异步响应绑定本地 turn ID，切换文档或其他 Agent 修改模型不会把旧操作应用到新状态。请求期间正常文档轮询和 UI 操作继续运行。

预览期间不把旧版本的引用颜色套到候选拓扑。内核验证错误进入原有预览/失败处理；只有明确 Confirm 才提交，失败回滚及不确定写入结果的原有核对机制不变。

## Agent 适配

新增 `cad-agent` Rust crate：跨平台引用/附件/Turn/Reply 数据结构、校验，以及原生宿主薄适配。宿主从真实场景补充毫米单位、三角网格包围盒、选中边的长度与离散曲线作为上下文；网格包围盒不宣称是精确 B-Rep 查询。

本环境 Web 发布选择已安装 **Pi**，复用其账户和默认模型配置。不读取、复制或返回登录凭证。其他部署默认禁用，须由宿主显式选择：

```bash
CAD_AGENT_BACKEND=pi CAD_AGENT_PI="$(command -v pi)" target/release/cad-gui
# 可选 CAD_AGENT_MODEL=provider/model，未配置时使用 Pi 自己的默认设置。
```

Pi 在独立0700临时目录、无会话保存模式运行；关闭工具、MCP、扩展、Skills、模板和项目上下文加载，真实推理结果仅能成为经校验的操作建议。附件使用生成文件名传递，用户文本经 stdin 输入，不拼接 shell；结束/失败/超时清理临时文件。180秒宿主超时及128KiB输出限制。此为工具禁用和受限建议协议，不是操作系统进程沙箱。

其他 Agent 可实现 **json-stdio** 适配器：宿主环境设置 `CAD_AGENT_BACKEND=json-stdio`，`CAD_AGENT_COMMAND_JSON` 为固定 argv JSON 数组。程序从 stdin 读取到 EOF 的单个 JSON 文档（`instructions`、`turn`、权威 `state` / `geometry`），stdout 只返回一个 Reply JSON，退出0表示成功。浏览器不能设置命令行、可执行文件或任意服务器文件路径。

Reply：

```json
{"text":"建议做 1 mm 倒圆，请预览确认。","proposal":{"kind":"fillet","reference":"本轮某个 Edge 的精确 target","radius":1}}
```

或 `proposal: {"kind":"box","size":[10,10,10]}`，或 `proposal:null` 用于普通回答、澄清和不支持的请求。拒绝未知操作、额外操作参数、伪造边和无效数值。变半径倒圆、凸台、拉伸、薄壳、布尔等当前未接入的操作必须明确说明，不通过关键词规则或定半径近似伪装实现。

Web `/api/agent` 继承原来的 Origin、会话 Cookie、页面 ID、固定 CAD service/session 授权；最多2个并发推理。只返回说明和建议，不在 HTTP handler 自动执行建模。MCP、CLI 核心命令协议保持不变。

## 测试

```bash
PATH="$HOME/.cargo/bin:$PATH" scripts/build-web.sh
PATH="$HOME/.cargo/bin:$PATH" cargo test --workspace --locked
CAD_PLAYWRIGHT=/path/to/playwright ctest --test-dir build -R '^web_agent_references$' --output-on-failure
CAD_PLAYWRIGHT=/path/to/playwright CAD_AGENT_LIVE=1 node tests/web_agent.cjs
```

`desktop_agent_references` 另用隔离 Xvfb 驱动真实桌面窗口，验证原生 Bridge、树引用、进程回复、预览/确认和过期高亮。

`web_agent_references` 使用真实 Rust 宿主和确定性子进程协议样本，覆盖四类引用、文本/图片传输、横竖屏布局、指定边倒圆预览/确认、Box建议预览/取消、旧引用与伪造建议拒绝、推理中并发修改及跨源拒绝。`CAD_AGENT_LIVE=1` 单独调用已登录 Pi 做真实推理，不作为常规离线 CI 的模型依赖；所有建模均在独立临时 CAD 服务中。
