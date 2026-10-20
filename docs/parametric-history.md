# 参数化文档、编辑历史与重计算（方案 D）

2026-10-20。实现于自有 C++ Commander / cad-document，SMLib 源码未改；CLI、MCP、桌面与 Web 仍是 Rust 接入层。第一版支持单实体 Box / 单边定半径 Fillet 顺序特征链。数据显式记录稳定 Feature ID、依赖和缓存结果；先不开放任意图重排、多实体引用、表达式或跨文档依赖。

## 两套独立的数据

- **特征定义**：`id/name/kind/dependencies/parameters/selector/suppressed/status`。修改名称不改变 ID，也不计算几何。Box 仍保留“替换当前单实体”的既有语义，新的 Box 创建新特征链；旧链保存在 Undo 状态中。
- **编辑历史**：不可变 `DocumentSnapshot` 保存整份特征定义、各节点几何结果和最终 B-Rep。历史游标与对外 revision 独立；Undo/Redo 切换已有状态，对外 revision 始终递增，不重放内核操作。编辑尺寸是新的事务，不是删除后续特征。撤销后新提交截断当前 redo 分支；失败操作不会截断。

每次 Commander 写入先复制文档元数据，仅对候选几何做深拷贝/重新计算。参数修改从被改节点开始重算其线性依赖后继；前缀缓存复用。所有节点成功且 B-Rep 验证有效后，构造并验证响应，持久提交，再用 noexcept swap 发布内存状态。几何、特征定义与历史一起发布。失败信息包含特征 ID；上一个有效模型及 redo 分支保留。

这是面向依赖图的节点/输入模型，当前调度器只开放并验证线性单输入 DAG；不声称已经实现任意 DAG 的多输入调度。抑制节点传递输入几何；抑制根节点导致仍启用的后继无输入时，整次修改失败。全部抑制可得到空文档。界面没有把失败参数标成已提交参数。

## 拓扑选择

对位于 Box 命名边界的轴向直边，保存相对于根 Box 的两个边界坐标和方向，不保存遍历序号；同一直线端点因邻接圆角缩短仍可解析。重算时必须唯一匹配，零匹配返回 `reference_unresolved`，多匹配返回 `reference_ambiguous`，不按旧 slot 猜测。

其他边保存规范坐标系下的完整离散几何描述，只允许严格容差内的唯一对应；参数变化后可能失效。它是有边界的精确几何匹配，不是通用 Persistent Naming。SMLib split/merge lineage、任意曲边跨尺寸变化、引用修复编辑器尚未实现。对外 `persistent_naming` 仍为 false。现有瞬时视图 token 继续绑定 document/revision；稳定 Feature ID 不等同于稳定 Edge ID。

## 原子 Agent 任务

`transaction.begin` 在当前 revision 上创建不进入 document.list 的私有文档。Agent 在其中读取新状态、添加/编辑特征及验证；阶段结果只供 Agent 和任务进度使用，普通共享视图继续显示已提交模型。成功时 `transaction.commit` 检查原文档 revision，自动把最终候选作为一个文档状态提交；一次 Undo/Redo 对应整个任务。无用户审批按钮。

失败通过 `transaction.abort` 丢弃候选，不再用多次公开 Undo 模拟回滚。并发修改导致提交冲突，不覆盖其他客户端。任务完成后若已有后续公共编辑，“Undo Agent task”拒绝跨过它们；全局 Undo/Redo 仍按文档提交顺序工作。聊天面板的任务结果是执行收据；手动 Undo/Redo 后以文档历史为准，旧任务收据不会被当作当前特征状态。旧 worker 不支持事务时保留已有行为，并在任务状态 `atomic=false` 明示。页面刷新不影响后台任务；网关进程重启会中断任务，恢复时不盲目重放未确认写入，并尝试丢弃仍存在的私有候选。私有事务在内核进程重启后丢弃，已提交文档从存储恢复。

Agent 新增 `feature_edit` 提案，可修改 name、suppressed、Box size、Fillet radius；宿主校验当前特征 ID、参数 schema 及显式特征引用约束。原 `event:<id>` 是旧文档操作记录，仅能提供上下文。

## 持久化与恢复

每个 owned worker 通过 `CAD_DOCUMENT_STORE` 独占自己的存储目录。Rust 服务按默认会话和命名会话分目录；`CAD_DOCUMENT_ROOT` 指定根目录，未指定时使用 runtime-dir/documents。生产发布配置为 `~/.local/share/miocad/documents`，不依赖 /run 跨重启保留。

存储格式 v1：不可变 `.smb`、对应显示/拓扑元数据、一个原子替换的 `workspace.json`。元数据保存原网格、边序和验证值，避免 SMLib 原生重载后重新三角化改变拾取结果。恢复验证实际 B-Rep 的有效性、体积/拓扑计数及定义/依赖结构；后续编辑恢复模型时，按唯一几何对应把已保存边映射到原生边，不按旧指针/序号盲用。缺文件、不兼容版本、无效数据会拒绝启动，不偷偷创建空文档。

提交先写入并 fsync 新几何及元数据，再写入并 fsync 临时 manifest，同步目录、原子 rename，最后同步目录。rename 后若目录同步失败，发布对应内存状态并返回 `outcome_unknown`，不能报告“修改被拒绝”或自动重放。native 保存或 rename 前失败不发布。保存文件记录固定内核版本；不隐式用新版本重算旧状态。

`document.save` 保存为 `.miocad` 目录项目，包含完整历史和缓存；`document.open` 在新文档 ID 下加载，原文档不受影响。Web RPC 禁止任意文件路径的 save/open，与既有 export 限制一致；暂未加入浏览器项目上传/下载 UI。

边界：存储格式未提供跨进程多写者合并、内容寻址去重、垃圾回收或可配置内存淘汰；不可达几何文件暂保留。manifest 上限64 MiB，几何512 MiB，单个几何元数据64 MiB，最多10000个历史状态，超限拒绝提交。必须整体备份项目目录，不是只复制 workspace.json。

## 兼容在线旧文档

旧进程只有几何快照，无法凭空恢复完整参数特征定义。发布不停止旧文档拥有者，不把它的操作日志伪装为特征链，也不丢弃原 Undo/Redo。

兼容 relay 在同一外部会话下按文档路由：原文档仍由原 worker 管理；新文档由新版持久 worker 管理。合并列表与事件，沿用统一 CLI/MCP/UI。旧文档界面明确标注 Legacy，继续原有建模/Undo/Redo；新文档显示真正特征和 Edit 按钮。新版拥有者已支持特征时不会额外创建第二个 worker。兼容 relay 重启会从磁盘恢复新文档，原拥有者继续存活。

旧文档不自动获得参数编辑或跨重启持久历史；需要后续显式迁移设计。原内存 worker 的全量历史没有只读导出接口，本次没有用破坏性的逐次 Undo 抓取历史。兼容通道的请求序号、逻辑客户端收据保留，断线重试不得重复提交。

## 验证

- `parametric_features`：真实 Box + 两个平行 Fillet、上游尺寸编辑、半径编辑、抑制、重命名、失败保留模型/redo、私有多步提交、一次撤销/重做、并发冲突、保存/重启/打开、写入故障及缺几何拒绝恢复。
- `feature_cache`：计数内核证明 Undo/Redo 和重命名不执行建模，编辑后继不重建前缀，编辑根重算下游。
- Rust 服务测试：崩溃/重启后文档与 revision 恢复，但服务/worker 身份更新，旧请求不可自动重放。
- `web_feature_editing`：真实 egui/WASM 参数表单、Box/Fillet 编辑、失败回滚、抑制、Undo/Redo、Agent 特征编辑任务及一次撤销、竖屏布局。
- `web_agent_references` / `desktop_agent_references`：既有自主任务、引用、附件、内核反馈、撤销及断线语义回归。
- `tests/hybrid_upgrade_test.py`：可选 `CAD_LEGACY_SERVER` 指定历史二进制，在临时服务验证兼容路由及重启；不访问生产文档。

本轮回归也修复了快速编辑中的截图请求竞态：Web Worker 的 HTTP 错误读取 status 属性；错误携带视图 epoch，过期视图的失败不能覆盖当前特征错误。此行为有共享 UI 单元测试，特征编辑浏览器测试检查最终帧及 model_error。

## 本轮执行结果

- 完整既有 CTest 套件 **21/21** 通过（222.47秒，包含桌面、WASM、触屏/拾取、MCP真实宿主发现及 Rust workspace）。新增兼容升级测试随后独立运行 **2/2**（含构建 fixture），使用隔离的历史 worker 二进制。
- Rust workspace **48项测试**通过；原生全目标及 WASM Clippy `-D warnings` 通过。计数内核、参数化/存储测试在最终 SMLib 状态缓存变更后再次通过。
- 浏览器实际 WebGPU 竖屏特征编辑及 Agent 特征编辑/任务 Undo/Redo 通过；已查看截图，修正特征操作按钮的对比度，并确认最终视图无错误。
- 已通过现有持久服务发布到 `https://example.ts.net/apps/miocad/` 并保持门户收录；HTTPS资源、免解锁、刷新和嵌套路由通过。
- 线上新建 **Document 03** 示例：12×10×10 Box + 半径2 Fillet，revision 8；验证 Box 修改带动圆角重算、圆角编辑及对应 Undo/Redo，存储 manifest 已落盘。没有调用线上 LLM 做验证。
- 原 Document 01（r26）与 Document 02（r4）的完整状态保持不变；原 worker/session ID 和拥有进程 **PID 673288** 保持不变。未迁移或丢弃它们的旧历史。
- 部署证据：忽略目录 `artifacts/parametric-deployment/`；自动化测试脚本与协议/实现均纳入 Git，未向远端推送。
