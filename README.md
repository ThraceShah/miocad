# AI-CAD

面向任意 Agent 的 Headless-First 精确 B-Rep CAD 原型。核心是独立 C++ 服务；
CLI 与 MCP 使用同一命令协议，没有专用 LLM 或特定 Agent 依赖。内核使用 SMLib，
公共 Shape / IKernel 接口不包含第三方类型。

## 已确认的改造方向

默认共享 CAD 服务，Agent/MCP/CLI、桌面 GUI 和 WebUI 为平级入口；C++ 核心抽取 Commander，
对外服务及客户端统一使用 Rust，桌面/Web 采用 egui / eframe，视口采用 wgpu。
Python 仅保留为内部脚本和测试工具。**P1 Commander 抽取已完成**；Rust 对外组件、共享服务及 GUI 尚未实现，下方仍使用现有 Python 入口。
详见 [架构决策](docs/architecture.md) 和 [分阶段实施计划](docs/implementation-plan.md)。

## 当前状态

已完成：官方 SMLib 调查、固定版本、原版内核独立编译、能力探针、九模块 MVP 架构，
以及可运行的 Box → 截图 → CPU 像素拾边 → Fillet → 有效性检查 → 结果图 → SMB 示例。
支持独立文档、修改失败回滚、Undo/Redo、过期引用拒绝，以及本地 CLI / MCP 接入。
新增可独立链接的 C++ Commander、核心命令发现与分类错误；worker 仅处理 JSONL 收发。

**后续工作**：GPU Picking、跨编辑 Persistent Naming、磁盘文档版本恢复。
STEP 导出不再属于项目要求；不引入 OCCT 备选内核。当前支持 SMLib 原生 `.smb` 导出。
Boolean 已在原版内核探针验证，尚未暴露为文档命令。
当前渲染为 CPU 预览，支持 shaded / face-id / edge-id PNG；像素选取为近似预览拾取。

## 构建与测试

需要 Git、CMake ≥ 3.20、Ninja、C++17 编译器和 Python ≥ 3.10。Python 仅使用标准库。
不需要显示服务器、GPU、OpenUSD、Python CAD 包或 Agent 账号。
本机验证 Linux x86_64 / GCC 15.2；其他平台未验证。

```sh
git submodule update --init --recursive
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build -j 6
ctest --test-dir build --output-on-failure
./build/kernel_probe
python3 examples/box_fillet.py
```

结果位于 `artifacts/demo.json`、PNG 图和 `artifacts/box-fillet.smb`。
示例使用一个可见 edge-ID 像素进行确定性自动选择；没有运行 VLM 推理。
已有 VLM 的 Agent 可读取 shaded 图，提供自己的像素坐标调用 `render.pick`。

## 运行服务 / CLI

```sh
./build/cad-server
# 或含 CPU 截图和像素拾取的 CLI 薄层：
python3 modules/cad-cli/main.py
```

每行输入一个 JSON，立即返回一行 JSON；同一个进程保留文档状态。先发送：

```json
{"op":"capabilities"}
{"op":"commands.list"}
{"op":"document.create"}
```

然后用返回的 document ID 创建模型，revision 是上一状态版本：

```json
{"op":"model.box","document":"<document>","revision":0,"size":[10,10,10]}
{"op":"topology.edges","document":"<document>"}
{"op":"render.snapshot","document":"<document>"}
{"op":"render.pick","render_id":"<render_id>","x":192,"y":110}
{"op":"model.fillet","document":"<document>","revision":1,"edge":"<reference>","radius":1}
{"op":"document.state","document":"<document>"}
{"op":"export.native","document":"<document>","path":"artifacts/result.smb"}
```

`render.snapshot/pick` 由 Python CLI/MCP 薄层执行，其他命令由 C++ 服务执行。
不同进程的文档相互独立；CLI 和 MCP 默认各自启动 worker，不共享同一个文档会话。
退出进程后内存历史消失。自定义构建目录可设置 `CAD_SERVER=/absolute/path/cad-server`。
导出父目录需已存在；snapshot 会创建其输出目录。

## MCP

```sh
python3 /absolute/path/ai-cad/modules/cad-mcp/server.py
```

任意支持 MCP stdio 的 host 可配置：

```json
{
  "mcpServers": {
    "ai-cad": {
      "command": "python3",
      "args": ["/absolute/path/ai-cad/modules/cad-mcp/server.py"]
    }
  }
}
```

具体 host 的配置文件位置自行选择，不需要修改内核。工具 `cad_command` 的参数为
`{"command":{"op":"...",...}}`。MCP 基线版本为 2025-11-25；已测试本地 JSON-RPC
握手与工具调用，尚未在真实 Claude Code / Codex / Pi / DSH 客户端逐一验证。

实现参考 [MCP stdio](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)、
[生命周期](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle) 和
[Tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)。

## 工程记录

- [当前项目范围与验收要求](docs/scope.md)
- [SMLib 调查、许可证、平台、生命周期与命名](docs/investigation.md)
- [共享服务、Commander 与 Rust/egui 架构决策](docs/architecture.md)
- [改造实施计划与验收门槛](docs/implementation-plan.md)
- [Commander 原生接口与事务边界](modules/cad-commander/README.md)
- [协议和失败语义](docs/protocol.md)
- [Agent 使用指南](modules/cad-skills/GUIDE.md)
- [验证结果与已知限制](docs/validation.md)
- [第三方说明](THIRD_PARTY.md)

尚未选择本项目原创代码的开源许可证；第三方许可证不自动覆盖原创代码。
没有配置远端，也没有推送。完整开源发布前需确定主许可证。
