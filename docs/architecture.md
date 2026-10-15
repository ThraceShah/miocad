# MVP 架构 / ADR-001

状态：首轮设计；SMLib 为候选内核；进程内串行、进程间并发；毫米单位。

```text
任意 Agent → MCP stdio / CLI → JSON command → cad-server
                                       → cad-modeling → cad-document → cad-kernel
                                       → cad-topology                 → SMLib
                                       → cad-render ← neutral Mesh / Edge polylines
```

| 模块 | 职责与边界 |
|---|---|
| cad-kernel | IKernel / Shape 接口；唯一包含第三方类型的实现；创建、copy、fillet、validate、mesh、native export；STEP 由 capability 协商 |
| cad-document | 独立文档，单 body 首版；不可变快照、copy/validate/commit；undo/redo；revision 单调递增 |
| cad-modeling | 有单位、范围、版本前置条件的建模命令，不把第三方调用透传给 Agent |
| cad-topology | 文档/revision/entity token，几何摘要；首版过期即拒绝，后续加入 lineage resolver |
| cad-render | 中性网格、face 标签、edge 折线；headless 截图和拾取；首版 CPU，后续 GPU ID buffer |
| cad-server | 独立 C++ 进程、JSONL 命令、多个文档、结构化错误；不依赖任何 LLM |
| cad-mcp | stdio JSON-RPC MCP 薄桥，工具发现与调用，复用相同服务命令 |
| cad-cli | stdio batch 与 demo，便于任意 Agent 直接调用与组合脚本 |
| cad-skills | 统一建模指南、引用有效期、失败处理、host 配置示例 |

## 状态与事务

每个 Document 拥有不可变历史 Shape 快照。修改先创建候选，深拷贝当前 BRep，
在候选上修改并校验，成功后一次提交。失败不更新 body、revision、历史或 redo 分支。
undo/redo 恢复快照并递增 revision；新编辑只在成功时截断 redo。
实体引用包含 document + revision + kind + token；即使 undo 回相同几何也不能接受旧 token。

首版 revision 是进程内历史版本，不是磁盘持久化版本管理。完整版本管理后续需要
命令日志、内容校验和、原子 snapshot 写入、重启恢复和格式版本迁移。
SMB 是内核相关格式，不是长期中性文档格式；STEP 是外部交换，不保存文档命令历史。

## 选择与图像

图像返回 document/revision、camera、viewport 和 render ID，拾取必须使用相同渲染上下文。
普通 shaded 图用于 VLM 定位，ID 图用于精确像素选取，几何查询用于消歧。
GPU 不是几何真值源：结果需映射回当前 revision 的 topology token，支持遮挡/容差规则。
首轮 CPU 预览只能验证接口与模型外观，不宣称 GPU picking 已完成。

## 失败与服务隔离

参数检查、过期引用、内核异常、验证失败均返回结构化错误。事务隔离保护可恢复的失败；
它不保护内核段错误或进程崩溃。完整服务需 worker 超时、资源限制、日志/快照恢复和监督进程。
首轮仅本地 stdio，文件导出遵从调用者本地权限，不暴露网络监听端口。

## 后续准入门槛

1. STEP import/export：对原生解析面、NURBS、闭合周期面和 fillet 做独立 roundtrip；
   校核 bounds、volume、topology、tolerances，必要时用 OCCT 作独立验证器。
2. Naming：验证通知覆盖；记录 generated/modified/deleted lineage；对不确定映射返回歧义。
3. 渲染：无显示 GPU backend、整数 face/edge ID buffer、深度一致 picking，固定 camera 回归。
4. 完整八步 Agent 工作流的 MCP 集成测试；至少两种 host 接入同一个协议。
5. 多 body / Boolean 文档命令、磁盘版本管理、内核崩溃恢复、跨平台 CI。
