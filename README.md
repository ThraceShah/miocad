# AI-CAD

面向任意 Agent 的 Headless-First 精确 B-Rep CAD 原型。核心是独立 C++ 服务；
CLI 与 MCP 使用同一命令协议，没有专用 LLM 或特定 Agent 依赖。首选候选内核 SMLib，
公共 Shape / IKernel 接口不包含第三方类型。

## 当前状态

已完成 SMLib 技术调查、原版内核构建、九模块 MVP 架构，以及可运行的
Box → Shaded 截图 → CPU 像素拾边 → Fillet → 验证 → 结果图 → 精确 STEP 导出/导入复核。
支持独立文档、修改失败回滚、Undo/Redo、过期引用拒绝，以及本地 CLI / MCP 接入。

默认仍为 SMLib（原生 SMB）；可选 OCCT 7.9+ 后端提供 STEP 和原生 BRep。
选择后端需显式传参，capabilities 始终报告实际内核；不在后台静默切换。
**完整第三阶段尚未完成**：GPU Picking、跨编辑 Persistent Naming、磁盘文档版本恢复仍未实现。
CPU 渲染支持 shaded / face-id / edge-id PNG，像素选取为近似预览拾取。
Boolean 已在原版 SMLib 探针验证，尚未暴露为文档命令。

## 构建与测试

需要 Git、CMake ≥ 3.20、Ninja、C++17 编译器和 Python ≥ 3.10。Python 仅使用标准库。
不需要显示服务器、GPU、OpenUSD、Python CAD 包或 Agent 账号。
本机验证 Linux x86_64 / GCC 15.2；其他平台未验证。

```sh
git submodule update --init --recursive
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release -DCAD_ENABLE_OCCT=OFF
cmake --build build -j 6
ctest --test-dir build --output-on-failure
./build/kernel_probe
python3 examples/box_fillet.py
```

结果位于 `artifacts/smlib/demo.json`、PNG 图和 `artifacts/smlib/box-fillet.smb`。
示例使用一个可见 edge-ID 像素进行确定性自动选择；没有运行 VLM 推理。
已有 VLM 的 Agent 可读取 shaded 图，提供自己的像素坐标调用 `render.pick`。

## 启用 OCCT / STEP

已有 OCCT 7.9+ 开发环境时，使用 `-DCAD_ENABLE_OCCT=ON`；必要时指定 `-DOCCT_ROOT=<prefix>`。
本机 Ubuntu 26.04 使用官方包 7.9.2+dfsg-4，以下路径已验证：

```sh
# 下载并解包到工作区，不安装系统包；需要 apt 源有相应版本。
python3 scripts/bootstrap_occt_debian.py
cmake -S . -B build -G Ninja -DCAD_ENABLE_OCCT=ON -DOCCT_ROOT="$PWD/build/occt-sdk/usr"
cmake --build build -j 6
ctest --test-dir build --output-on-failure
python3 examples/box_fillet.py --kernel occt
```

结果：`artifacts/occt/box-fillet.step`、`.brep`、PNG 和 `demo.json`。
示例把 STEP 导入一个新文档，再核对有效性、体积和 bounds。输入支持单个 solid；
开放面、多实体/装配和混合游离几何会明确拒绝。STEP 不保存文档历史。
SMLib 的 STEP capability 仍为 false，不能将 OCCT 支持理解为已实现 SMLib 格式转换。
本地 SDK 构建使用 build RPATH，不应删除其 SDK 后期待动态链接的可执行文件继续工作。

## 运行服务 / CLI

```sh
./build/cad-server
# 或含 CPU 截图和像素拾取的 CLI 薄层：
python3 modules/cad-cli/main.py
# 启用 STEP（需要 OCCT 构建）：
python3 modules/cad-cli/main.py --kernel occt
```

每行输入一个 JSON，立即返回一行 JSON；同一个进程保留文档状态。先发送：

```json
{"op":"capabilities"}
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
导出父目录需已存在；snapshot 会创建其输出目录。`CAD_KERNEL=occt` 可作为后端默认值，
显式 `--kernel` 优先。OCCT 模式可使用：

```json
{"op":"export.step","document":"<document>","path":"artifacts/result.step"}
{"op":"import.step","document":"<target-document>","revision":0,"path":"artifacts/result.step"}
```

`import.step` 会替换目标文档 body，须给当前 revision；校验失败保持原文档和 redo 不变。
原生导出扩展名按后端区分：SMLib 用 `.smb`，OCCT 用 `.brep`。

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
      "args": ["/absolute/path/ai-cad/modules/cad-mcp/server.py", "--kernel", "occt"]
    }
  }
}
```

此配置启用 OCCT；SMLib-only 构建请改为 `smlib` 或省略参数。具体 host 的配置文件位置自行选择，不需要修改内核。工具 `cad_command` 的参数为
`{"command":{"op":"...",...}}`。MCP 基线版本为 2025-11-25；已测试本地 JSON-RPC
握手与工具调用，尚未在真实 Claude Code / Codex / Pi / DSH 客户端逐一验证。

实现参考 [MCP stdio](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)、
[生命周期](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle) 和
[Tools](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)。

## 工程记录

- [SMLib 调查、许可证、平台、生命周期、命名与 OCCT 对比](docs/investigation.md)
- [MVP 架构与技术决策](docs/architecture.md)
- [OCCT/STEP 决策与数值回归](docs/adr-002-step-backend.md)
- [协议和失败语义](docs/protocol.md)
- [Agent 使用指南](modules/cad-skills/GUIDE.md)
- [验证结果与已知限制](docs/validation.md)
- [第三方说明](THIRD_PARTY.md)

尚未选择本项目原创代码的开源许可证；第三方许可证不自动覆盖原创代码。
没有配置远端，也没有推送。完整开源发布前需确定主许可证。
