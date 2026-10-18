# P5：egui 桌面工作区

2026-10-18。实现为 Rust `cad-gui`（原生宿主/异步连接）和 `cad-ui-egui`（界面、读模型、交互状态），
共用 cad-client / cad-protocol / cad-render。GUI 不调用 SMLib，也不通过 MCP 绕行；它与 CLI/MCP 平级。

## 启动

```sh
cmake --build build -j 6
cargo build --workspace --locked
./target/debug/cad-gui
# 与指定 Agent/CLI 共享实例：
./target/debug/cad-gui --runtime-dir /tmp/cad-workspace
./target/debug/cad-cli --runtime-dir /tmp/cad-workspace
```

参数复用 `--runtime-dir`、`--service`、`--worker`、`--client-id`、`--session`、`--isolated`、`--help/--version`。
GUI 不接受 --command。默认发现/启动同一用户的服务；关闭窗口只断开该客户端，文档仍在 daemon 内存中。
重开窗口或连接 MCP/CLI 可继续使用。要停止整个服务，应显式调用 service.shutdown。
单个逻辑 client ID 同时只用于一个活跃入口；默认每窗口生成新 ID，相机/选择/输入草稿均不在服务共享。

release 包现在包含五个程序：cad-gui、cad-cli、cad-mcp、cad-service、C++ cad-server。
四个无界面程序仍不需要显示服务器；GUI 需要桌面显示和可用的 wgpu Vulkan/GLES 适配器。
仅验证 Linux x86_64 / X11；本次 Xvfb 验收使用 Mesa llvmpipe Vulkan 软件适配器，没有验证真实显卡或其他操作系统。

升级原有 P4 服务前，先完成需要的 SMB 导出并显式关闭旧服务，再启动匹配版本的 worker/service/GUI。
不自动替换仍持有内存文档的旧进程，也不把旧请求迁到新服务。

## 交互

- 左侧列出当前会话的文档及单个 body 摘要；选择文档不会改变其他入口的“当前文档”。
- New document 创建空文档。Box 提供 Width/Depth/Height，Fillet 通过点击视口可见边选择并设置半径。
- Preview shape 在临时文档副本中计算候选，显示候选及体积；正式文档与历史不变。
  Confirm 使用预览捕获的 document/revision/参数提交一次原建模命令；重复点击不会重复提交。
  Cancel 或修改参数丢弃候选；已经进入内核的预览计算可能继续，但迟到结果不会覆盖新草稿。
- Undo/Redo 的可用性来自核心原子场景查询，操作后使用新的 revision。
- 拖动视口旋转，Shift/右键拖动平移，滚轮缩放，双击复位；只改变本窗口相机，既不提交文档也不产生文档事件。
  边选择/悬停仅本窗口有效，Agent 必须显式传自己的 edge/reference。
- Native export 可输入已有父目录下的 .smb 路径。Copy ID 便于与 Agent 协调显式文档 ID。
- 服务提交事件每 450ms 拉取；发现变化/游标过期即刷新权威快照，再按版本读取活动场景。
  其他入口改动会清除过期边选择、作废候选并提示 Preview stale，不自动修改预览版本后重试。
- 断连时显示 Offline、保留最后读取的场景，禁止提交。Reconnect 只核对同一 service/worker。
  不确定请求保留原 Request，通过 request.status 核对后刷新；缓存不可得时要求检查刷新后的状态，绝不自动重做。
  daemon 已重启/worker 已丢失时，旧请求不迁移；显式重新打开或选择新实例处理新的文档。

## 线程与视口

UI 绘制线程只处理 egui、客户端读模型和输入，不等待 CAD/网络/CPU rasterization。
后台 Tokio 线程串行发送业务命令与拉取事件；另一个渲染线程只处理最新排队的相机/场景，旧 epoch 的图像不会覆盖新状态。
命令和结果通道上限为 32，视口队列合并到最新任务；GUI 关闭后后台结束连接，不管理共享 daemon 的寿命。

P5 使用共用 CPU raster/pick 映射生成交互纹理，由 eframe/wgpu 显示；没有宣称 GPU 几何渲染或 GPU Picking。
本地视口采用稳定的世界包围球作为 orbit 中心和缩放尺度，选择高亮来自同一像素引用映射。
CLI/MCP 的 render.snapshot 仍是无窗口场景截图，GUI 相机不会改变 Agent 的截图或文档 revision。

## 核心新增的只读能力

`render.scene(document)` 原子返回 state、mesh、edges、can_undo/can_redo，解决 UI 分别查询网格与边时的版本竞争。
空文档返回 mesh=null；语义不同于仅非空 body 的 render.mesh。

`model.preview(document, revision, operation)` 中 operation 为：

```json
{"kind":"box","size":[10,10,10]}
{"kind":"fillet","edge":"<live reference>","radius":1}
```

Commander 复制 Document 元数据，复用已有 Box/Fillet 处理器和几何克隆/验证，查询完候选即销毁。
外层 document/revision 指向基础正式文档；data.state 是候选状态，包含假设提交后的 revision。
候选 mesh/edge 引用使用独立临时命名空间，不能用于修改正式文档；确认后必须重新读正式场景。
没有建立第二套 Rust 建模算法、预览文档清单或假提交再 Undo 的流程。
失败/取消/查询/相机操作均不截断正式 redo 分支。

## 验证

```sh
# 普通 headless 回归，不要求显示服务器：
ctest --test-dir build --output-on-failure
# 完整 P5：增加真实原生窗口验收（Linux，要求 Xvfb、ffmpeg、xwininfo、libX11/libXtst、Mesa Vulkan/GLES）：
cmake -S . -B build -DCAD_ENABLE_GUI_TESTS=ON
ctest --test-dir build --output-on-failure
# 或独立运行原生窗口验收：
python3 tests/gui_smoke.py
```

原生测试创建自己的 Xvfb，不操作用户桌面。通过 XTest 点击真实控件、旋转视口、关闭/重开窗口；
同时用独立 Rust CLI/MCP 查询/修改文档并断言 revision/体积/事件。PNG、SMB 与机器记录位于 artifacts/p5-gui/。
内部 gui_probe.py 仅是测试驱动，不随产品发布，也不是 CAD 服务入口。
生成包：`python3 scripts/package.py`；包内不含 Python，包含 GUI 默认字体的许可文本。

当前不包括 WebUI、GPU Picking、任务进度/内核抢占取消、持久 Naming、磁盘恢复、多 body 或特征树。
