# AI-CAD

面向通用 Agent 的 Headless-First 精确 B-Rep CAD 系统。C++ Commander / SMLib 管理权威文档和建模，
Rust 共享服务、CLI、MCP 与 egui 桌面/Web 提供统一接入；不绑定模型或 Agent，无界面路径不需要显示服务器。

**P1–P7 已完成**：Commander、中性协议、Rust 客户端、默认共享服务，以及 Rust CLI/MCP、共享 GPU/CPU 场景渲染和可运行的 egui 桌面/Web 工作区。
桌面、WebUI 与 Agent 为平级客户端，共享 wgpu/CPU 视口与 ID 拾取；磁盘恢复仍属后续工作。
Python 仅用于内部测试/打包，原型代码位于 tests/legacy，产品运行路径不依赖 Python。

支持文档创建/列举、Box、单边 Fillet、拓扑/网格查询、失败回滚、Undo/Redo、场景截图、像素拾取及 SMB 导出。
仅使用 SMLib，不恢复 STEP 导出或 OCCT 备选要求。Boolean 仅在内核探针验证，未作为文档命令开放。
已有交互预览、确认/取消；Persistent Naming、磁盘恢复仍属后续工作。

## 构建与验证

需要 CMake ≥3.20、Ninja、C++17、Rust 1.90.0；内部测试使用 Python ≥3.10 标准库。
工具链和 Cargo.lock 已固定。本机验证 Linux x86_64 / GCC 15.2，其他平台尚未验证。

```sh
git submodule update --init --recursive
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build build -j 6
cargo build --workspace --locked
ctest --test-dir build --output-on-failure
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
```

cargo 不在 PATH 时可使用 `$HOME/.cargo/bin/cargo`。CTest 包含 Rust workspace、产品构建和无 Python PATH 的 CLI/MCP 端到端验证。
仅构建 C++ 可设置 CAD_ENABLE_RUST_TESTS=OFF，不能据此宣称完整阶段验收通过。
`BUILD_TESTING=OFF` 的 C++ 构建不查找 Python；发布产物运行不需要上述构建工具。

## CLI 与共享服务

```sh
./target/debug/cad-cli
# 或单条命令
./target/debug/cad-cli --command '{"op":"document.list"}'
```

CLI 逐行读取 JSON、逐行返回 JSON。默认自动发现/启动同一用户的共享服务；CLI 和 MCP 使用相同
runtime-dir 即共享文档，入口退出后文档保留。`--help` 显示路径、身份、会话及隔离参数。

```json
{"op":"commands.list"}
{"op":"document.create"}
{"op":"model.box","document":"<document>","revision":0,"size":[10,10,10]}
{"op":"render.snapshot","document":"<document>"}
{"op":"render.pick","render_id":"<render_id>","x":192,"y":110}
{"op":"model.fillet","document":"<document>","revision":1,"edge":"<reference>","radius":1}
{"op":"document.state","document":"<document>"}
{"op":"export.native","document":"<document>","path":"artifacts/result.smb"}
```

snapshot 返回 shaded、face-id、edge-id 的 base64 PNG，默认 384×384。指定 directory 可额外保存 PNG，路径在 files。
这是 CAD 场景截图，不需要 GUI。pick 必须使用同一帧的坐标；过期帧拒绝，背景或歧义返回 null reference。
建模使用显式 document/revision/reference；不共享隐式“当前选择”。导出父目录需已存在。

`service.snapshot/events` 用于快照刷新和变化同步；`service.sessions` 列举隔离会话，`service.attach` 显式切换。
显式 `service.shutdown` 才关闭整个共享实例；内存文档不提供崩溃恢复。详见 [服务运行说明](crates/cad-service/README.md)。

Rust 闭环示例：

```sh
cargo run -p cad-access --example box_fillet --locked -- --runtime-dir /tmp/cad-demo
./target/debug/cad-cli --runtime-dir /tmp/cad-demo --command '{"op":"service.shutdown"}'
```

生成 artifacts/p4-demo/ 下的 PNG、SMB 和 demo.json。自动像素选择用于确定性验收，未运行 VLM。

## egui 桌面

```sh
./target/debug/cad-gui --runtime-dir /tmp/cad-workspace
```

CLI/MCP 使用同一 runtime-dir 即共享文档。桌面支持文档切换、Box/Fillet 参数、边选择、
临时预览/确认/取消、Undo/Redo、本地旋转/平移/缩放和提交事件同步。
关闭窗口不会关闭服务；相机/选择/草稿不会污染 Agent 状态。
当前由 eframe/wgpu 显示共用 CPU raster 的交互视口，并非 GPU Picking。
[运行、设计与真实窗口验收](docs/desktop-p5.md)。

## MCP 与发布包

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

工具名保留 `cad_command`，参数为 `{"command":{"op":"..."}}`。支持 MCP 2025-11-25 / 2025-06-18、
生命周期、工具发现、结构化错误与 PNG 图像结果。stdout 只输出协议内容；日志写 stderr。
已验证本地 JSON-RPC，尚未逐一验证各 Agent host 的真实安装配置。

```sh
python3 scripts/package.py
```

内部打包脚本生成 artifacts/dist/bin/ 的五个原生程序及第三方声明；将程序放在同一目录运行，
不需要源码、Cargo、CMake 或 Python。系统运行库仍需相容；GUI 需要显示服务器和 Vulkan/GLES，CLI/MCP 不需要。
旧 Python 启动路径会明确退出并提示迁移；配置替换、截图字段变化、资源限制见 [P4 迁移指南](docs/migration-p4.md)。

## 工程记录

- [架构与语言分层](docs/architecture.md)、[分阶段计划](docs/implementation-plan.md)
- [SMLib 调查](docs/investigation.md)、[范围](docs/scope.md)、[第三方说明](THIRD_PARTY.md)
- [核心命令与协议](docs/protocol.md)、[中性契约与重试语义](protocol/README.md)
- [Rust 共享服务](crates/cad-service/README.md)、[客户端 SDK](crates/cad-client/README.md)
- [Agent 使用指南](modules/cad-skills/GUIDE.md)、[验证记录](docs/validation.md)

## egui WebUI

入口由发布者自己的 Tailscale 主机名决定：`https://<host>.<tailnet>.ts.net/apps/miocad/`，门户为 `https://<host>.<tailnet>.ts.net/`。
本项目不在源码中内置任何具体部署域名，`scripts/publish-web.sh` 会从本机 tailnet 身份推导，也可用 `CAD_WEB_ORIGIN` 显式指定。
当前 Tailscale 发布环境直接进入工作区，无需输入访问码。`scripts/publish-web.sh` 显式设置 `CAD_WEB_TRUSTED_ACCESS=1`，浏览器仍使用隔离的会话 Cookie 和页面身份。

其他默认启动方式保留访问码：首次输入本机 CAD runtime-dir 的 `web-access-code`（0600 文件），默认路径 `/run/user/1000/miocad/web-access-code`。移除上述环境变量并重启 Web 网关，即恢复访问码模式。
网页和桌面/CLI/MCP 默认共享会话；相机、选择和草稿各自独立。网页导出使用 Download SMB。

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
scripts/build-web.sh
# 本地启动参数、授权边界及浏览器验收见 docs/web-p6.md
python3 scripts/package.py --web-only --output artifacts/web-bundle
# 按用户指定的 tailnet-publish 流程持久部署并收录门户
scripts/publish-web.sh
```

Web 网关和共享 CAD daemon 使用独立 systemd unit；重启网页不销毁共享文档。
CAD daemon 自身重启仍丢失内存文档。Web 通过客户端 wgpu 渲染缓存网格，优先 WebGPU、不可用时回退 WebGL2；旋转/平移/缩放不等待网络。停下后同步服务端 ID 帧用于拾取和截图，仍保留版本与相机校验。
详见 [P6 Web 说明](docs/web-p6.md)。

P7 新增 `render.frames` / `render.capture`，可取得界面所用的同一服务帧；`render.snapshot` 支持相机、revision 与 backend=auto/gpu/cpu。
headless wgpu 的 depth/Face ID/Edge ID 与 CPU fallback 已实际验证，渲染入口升级保留已有文档。
接口、数值边界和性能说明见 [P7 共享视口](docs/viewport-p7.md)。


触屏交互采用随手指方向旋转；边拾取支持触屏容差、青色悬停和琥珀色加粗选中。
`CAD_ENABLE_WEB_TESTS=ON` 时，`web_touch_and_picking` 在独立临时服务中回归触摸方向、九条可见边、旋转后的拾取、高亮像素及延迟帧。详细命令和边界见 [视口验证](docs/viewport-p7.md)。

对于微小倒圆后落在同一像素上的多条边，点击会弹出候选列表，支持触屏或数字键分别选择。`web_micro_fillet_picking` 固化此场景，样本和运行方式见视口验证文档。


平板工作区采用文档标签、顶部图标栏、可收起的左侧参数区和透明模型树。单指旋转、双指捏合缩放/移动平移；工具栏另有 Zoom ± 和 Fit。竖屏不再保留两侧常驻栏。操作与固化测试见 [平板布局](docs/tablet-ui.md)。

顶部 Agent 打开带命名边/面/体/特征引用的对话面板，支持文件和图片，本机 Agent 自主执行 Box / 定半径 Fillet 多步任务并验证结果，默认无需预览或审批，支持撤销整个任务。配置、引用版本边界与测试见 [Agent 工作区](docs/agent-workspace.md)。

## 许可证

本项目以 [MIT 许可证](LICENSE) 发布。

`third_party/` 下的第三方代码、字体及 Rust 依赖保留各自许可证，不受本项目 MIT 许可证覆盖。
分发二进制时需保留适用的版权与许可声明，详见 [第三方组件](THIRD_PARTY.md)。
