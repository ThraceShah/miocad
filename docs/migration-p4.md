# P4 运行与 Python 入口迁移

2026-10-18。产品入口现为 Rust `cad-cli` / `cad-mcp`，默认连接 Rust `cad-service`，
由其管理 C++ `cad-server` / Commander / SMLib。所有产品进程均无需 Python。
`cad-server` 仍是内部 worker，不应替代共享产品入口。

## 构建、运行和打包

```sh
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build -j 6
cargo build --workspace --locked
# CLI：每行一个命令，每行一个结构化结果
./target/debug/cad-cli
# 单条命令；失败时返回非零退出码
./target/debug/cad-cli --command '{"op":"document.list"}'
# MCP stdio
./target/debug/cad-mcp
# 内部构建脚本生成 release 产物及许可证清单
python3 scripts/package.py
```

目录及归档 `artifacts/dist/` / `artifacts/dist.tar.gz` 已包含所需文件。
包目录 `artifacts/dist/bin/` 包含四个原生程序：cad-cli、cad-mcp、cad-service、cad-server。
将四者放在同一目录，运行时无需源码、Cargo、CMake 或 Python；仍需要相容的 Linux 动态加载器、
glibc、libstdc++ 等系统运行库。SMLib 静态链接入 worker；只验证本机构建平台，不承诺任意发行版兼容。
打包脚本和测试是内部工具，Python 不复制到包内。项目本身的分发许可证尚未选定，沿用 THIRD_PARTY.md 的说明。

两个入口共用参数：

| 参数 | 含义 |
|---|---|
| --runtime-dir DIR | 选择共享实例发现目录；不指定时使用 XDG_RUNTIME_DIR/miocad 或 HOME/.cache/miocad |
| --service PATH / CAD_SERVICE | Rust daemon 路径，默认当前程序同目录 cad-service |
| --worker PATH / CAD_SERVER | C++ worker 路径，默认同目录 cad-server；源码开发时可回落到 build/cad-server |
| --client-id ID | 稳定逻辑客户端标识，用于同实例结果核对；同一时间只允许一个活跃入口使用此 ID |
| --session ID | 显式附着已有隔离会话 |
| --isolated | 按当前 client ID 的幂等 key 创建隔离会话，与 --session 互斥 |
| --help / --version | 本地帮助/版本，不连接服务 |

未指定 client ID 时按进程生成新 ID。CLI 和 MCP 使用相同 runtime-dir，默认访问同一份文档。
退出入口不会关闭服务；`{"op":"service.shutdown"}` 显式停止整个实例并丢弃内存文档。
`--isolated` 会话也由 daemon 保留，可通过 service.sessions 找到并 attach，不随入口退出销毁。
保留 P3 的资源上限：每服务 16 个会话，每 worker 128 个逻辑客户端（含内部观察者），不自动回收高水位记录。
升级已运行的旧 daemon 时，应先完成所需 SMB 导出，再显式关闭并启动新版本；不自动替换仍持有文档的旧进程。
自动启动只负责就绪发现；依旧不自动重放建模修改、不恢复崩溃后的文档。

## MCP 配置替换

将旧配置：

```json
{"command":"python3","args":["/path/miocad/modules/cad-mcp/server.py"]}
```

替换为：

```json
{
  "mcpServers": {
    "ai-cad": {
      "command": "/absolute/path/dist/bin/cad-mcp",
      "args": ["--runtime-dir", "/absolute/path/cad-runtime", "--client-id", "my-agent"]
    }
  }
}
```

仍使用工具 `cad_command`，参数仍为 `{"command":{"op":"..."}}`。
支持 MCP 2025-11-25 与 2025-06-18；其他版本按协商返回 2025-11-25，由客户端决定是否兼容。
要求 initialize → notifications/initialized；EOF 关闭 MCP 进程但保留共享服务。
只声明 tools，不声明 HTTP、resources、prompts、异步 tasks 或抢占取消能力。
协议/工具名错误通过 JSON-RPC error 返回；CAD 命令失败通过 isError=true 和结构化 error.kind 返回。
同时提供 structuredContent 和内容相同的 text；截图另附 shaded 的 image/png ImageContent。
普通通知不产生响应，tools/call 通知不执行建模。接受的内核调用不会因断连/取消而回滚或伪报取消成功。

依据 [MCP 生命周期](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)、
[stdio 传输](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports) 与
[工具结果](https://modelcontextprotocol.io/specification/2025-11-25/server/tools)。
使用内部协议测试完成兼容性验证，尚未逐一运行各 Agent host 的真实安装配置。

## 命令与截图变化

core op、mm 单位、document/revision、opaque reference、ok/result/error 布局和 `id` 回显保留。
产品入口拒绝未声明字段；`commands.list` 现在合并核心与服务/渲染目录，MCP inputSchema 从同一目录派生。
C++ worker 的 commands.list 仍只描述核心命令。
新增 service.info / sessions / snapshot / events / session.create / attach / reconnect / shutdown，
以及 request.status。参数和输出 schema 通过 commands.list 查询。

`render.snapshot` 默认 384×384，可指定 width/height（64..768）。返回 CAD 场景的
shaded、face-id、edge-id 三张 PNG，**images 的值现在是 base64 PNG，不再是文件路径**。
可选 directory 在服务端额外保存文件，绝对路径位于 files；省略 directory 不写磁盘。
返回 render_id、document/revision、worker/session、相机参数、尺寸和 ID→reference 映射。
这是无 GUI 的场景截图；窗口截图及 GUI 交互属于 P5。

`render.pick` 使用同一 render_id、整数 x/y，可选 target=edge（默认）或 face。
背景 reference=null；边容差区域重叠时 ambiguous=true、reference=null，换邻近像素或结合几何查询消歧。
不再像旧原型在重叠处静默取最后一条边。RGB ID 小端 24-bit，0 为背景/不可直接选中的歧义像素。
图像坐标原点左上；深度容差与旧 CPU 原型相近，不宣称 GPU/精确射线拾取。
frame 缓存在服务的对应会话内，可跨 CLI/MCP 使用；每会话最近 4 帧，淘汰/重启后需重新截图。
任何成功编辑或 Undo/Redo 使旧帧失效；pick 返回 stale_render。Fillet 仍以核心 revision/引用验证为准，
即使 pick 后另一客户端修改，Fillet 也不能绕过冲突检查。

传输不确定性返回 outcome_unknown，并携带原 worker Request；此时不要以新 ID 重做修改。
同一逻辑客户端调用 service.reconnect，再用 request.status 的 target_request_id 核对原请求，
必要时查询 service.snapshot / document.state。缓存过期只表示结果不可得，不表示命令未执行。
服务或 worker 身份变化拒绝旧请求；崩溃恢复需要显式创建新模型。

## Python 退出清单

| 原位置 | 当前用途/替代 |
|---|---|
| modules/cad-cli/main.py | 仅退出码 2 的迁移提示；服务实现已移到 tests/legacy/cli.py |
| modules/cad-mcp/server.py | 仅退出码 2 的迁移提示；服务实现已移到 tests/legacy/mcp_server.py |
| scripts/client.py | 删除产品路径，内部回归版本为 tests/legacy/client.py |
| modules/cad-render/preview.py | 移到 tests/legacy/preview.py；产品替代为 Rust cad-render + 服务共享缓存 |
| examples/box_fillet.py | 仅迁移提示；旧示例移到 tests/legacy/box_fillet.py |
| scripts/package.py、tests/*.py | 内部构建、打包与回归工具，不进入运行包 |

Rust 闭环示例：

```sh
cargo run -p cad-access --example box_fillet --locked -- --runtime-dir /tmp/cad-demo
# 验证产物 artifacts/p4-demo/ 下的 PNG、SMB 和 demo.json 后，显式关闭测试实例：
target/debug/cad-cli --runtime-dir /tmp/cad-demo --command '{"op":"service.shutdown"}'
```

示例的自动像素选择是确定性验收替身，不宣称已运行 VLM。原生 SMB 导出保留，未引入 STEP 或 OCCT。
