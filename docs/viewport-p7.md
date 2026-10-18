# P7：共享 GPU 视口与拾取

桌面、Web 和 Agent 使用服务端同一份不可变 Frame。C++ Commander/SMLib 继续拥有精确 B-Rep、事务、revision 和历史；
Rust cad-render 用 wgpu 27.0.1 在无窗口/无 Surface 环境执行两遍 compute raster：第一遍生成光照色、Face ID 和浮点深度，
第二遍读取深度生成 Edge ID、歧义候选。CPU 仅准备投影/法线光照属性；GPU 实际完成覆盖、深度排序及 ID 栅格化。
这不是把 CPU 图像上传到 GPU 后称作 GPU Picking。

桌面继续用 eframe/wgpu 显示纹理；Web 用 eframe/WebGL2 显示同一服务帧，不要求浏览器支持 WebGPU。
Web Worker 执行异步视口 HTTP 和数据处理，不在浏览器另算一张可能不同的几何 ID 图。无 GPU 时服务端可退回 CPU，
Frame.backend、adapter.device_type/name 和 fallback_reason 如实说明执行路径；wgpu 的 Cpu adapter 也明确报告为软件适配器。

## 公共命令

CLI 和 MCP 使用同一 cad-access 注册表及中性 schema。所有变更仍经原 Commander。

```json
{"op":"render.snapshot","document":"<doc>","revision":1,"width":384,"height":384,"backend":"gpu","camera":{"yaw":0.7853981633974483,"pitch":0.6154797086703875,"zoom":1,"pan":[0,0]}}
{"op":"render.frames"}
{"op":"render.capture","render_id":"<id>"}
{"op":"render.pick","render_id":"<id>","x":192,"y":245,"target":"edge"}
```

- backend=auto（默认）优先可用的 wgpu；cpu 显式使用 CPU；gpu 要求 wgpu 成功，否则报错，不静默回退。
  CAD_RENDER_BACKEND=cpu 可在服务启动时禁用 GPU，以验证/运行无 GPU 路径。
- 省略 camera 保留旧 snapshot 的紧凑 fit；显式 camera 使用和 GUI 相同的稳定包围球投影。
  相机、平移/缩放、尺寸为帧的一部分，不修改文档 revision。单位仍为 mm。
- render.frames 返回当前会话最近四帧的轻量元数据，包含各自 view_id/epoch 和预览标记，不建立全局“当前相机”。
- render.capture 返回指定已提交帧的原始 PNG/ID 映射；不重新渲染。UI 的选择高亮、悬停、轴标和窗口控件是本地叠加层。
- render.pick 使用同一 GPU 读回的 ID 平面。背景返回 null；重叠边返回 ambiguous=true、null 引用及候选，不能自动取第一个。
  GPU 最多列出三个候选，更多时 candidates_truncated=true；用 topology.edges 几何查询或换像素消歧。
- PNG 包含 shaded、face-id、edge-id、depth；深度 PNG 是归一化灰度可视化，实际遮挡使用 GPU 浮点深度，不能从灰度推算精确距离。
- Frame 带 document/revision、worker/session、render_id、相机、尺寸，以及 UI 的 view_id/epoch。
  同一视口更新后旧 epoch 的拾取/捕获被拒绝；同 epoch 改参数被拒绝；文档变化后旧帧失效；淘汰帧需重新拍摄。
- 预览通过 Commander.model.preview 重新计算候选，仅用于临时渲染，不能作为已提交帧 capture/pick，且不发布文档事件/历史。
  确认按钮要求当前候选图像已到达；迟到或尺寸/相机/版本不符的帧不会显示或启用确认。
- 已经取得的显式拓扑引用仍按文档 revision 验证；相机变化不会改变 B-Rep 身份，也不把几何命令绑定到 GUI。

边容差是屏幕内的 ±2 px 方形采样。深度比较利用可见三角形平面斜率修正采样位置，使用 0.05 px 的深度容差，
避免沿用大深度偏差而穿透薄表面。它仍是三角化视图拾取，不是无限精度的 B-Rep 射线求交；交汇/歧义区域返回候选。

## 传输、缓存及异步边界

cad-protocol::service::Viewport 为宿主中性 DTO，RenderView 返回同一 cad-render::Frame。
Web gateway 限定授权 service/session，并强制 view_id 为当前页面客户端身份，不能冒用其他页面或提交服务器路径。
原生渲染线程和 Web Worker 都合并待处理的最新视图；egui 只接收当前 epoch、document/revision、相机和尺寸匹配的结果。
渲染尚未到达时显示 Updating viewport，不展示旧图冒充新预览。

服务缓存最后一个文档/版本的完整 Scene，检查 document.state 后重用；编辑/Undo/Redo 自然使缓存失效。
GPU 对投影后的三角形/边字节作精确键比较，同一几何/姿态不重复上传。相机变化仍会重建投影和上传，这是当前明确边界。
帧缓存四份、视口身份最多 128；不为释放资源自动重启文档服务。服务串行持有对应 worker 的渲染发布过程，GPU/压缩在阻塞任务中执行。

RGB 和两个 u32 ID 平面通过有解压上限的 zlib/base64 传输，避免每帧数百万个 JSON 数字；最大 768×768，解码验证长度。
PNG 和元数据来自同一帧。render.frames 不携带大图；需要图像时显式 capture。MCP 仍以结构化结果加 image/png 内容返回。
P7 的渲染扩展需要配套更新 Rust 客户端/网关；内部建模协议和旧 C++ 文档拥有者可以保留。

## 不丢文档的服务升级

已有文档在内存中，不能为升级 GPU 直接重启 CAD daemon。新增：

```sh
cad-service --relay-runtime-dir /run/user/1000/miocad
```

此独立 Rust 入口持有自己的锁，将原 loopback endpoint 私密保存为 render-upstream.json，附着原来的 C++ worker，
仅将发现地址切到新监听器，保留原 protocol/service ID/token、worker/session 和请求 high-water。
普通 RPC、会话、快照和事件转发给原拥有者；GPU/帧缓存留在新入口。新会话按需附着，至多 16。
SIGTERM/正常停止会恢复原监听地址，不杀原 worker。重启渲染入口丢弃其帧缓存，需要重拍，不丢文档/Undo/Redo。
请求不确定结果仍在同一原 worker 上核对，不重放。SDK 的显式 reconnect 可跟随发现地址，但严格拒绝 service/worker 身份变化。

生产用三个独立 user systemd unit：miocad-cad-service（文档拥有者）、miocad-render-service（GPU/转发）、miocad-web（HTTP/WASM）。
scripts/publish-web.sh 更新后两者，不替换已有文档拥有者。Web 生产启动要求渲染入口已发布，避免开机时误附着原监听器；
独立手工重启渲染入口后，也须重启 Web 网关并重新解锁，原生/MCP 可显式 reconnect。若拥有者自身停止/崩溃，仍不具备磁盘恢复能力。
默认 HTTPS 入口仍为 https://example.ts.net/apps/miocad/，按 tailnet-publish 技能发布并收录门户。

## 验证与性能边界

- 确定性测试覆盖真实 headless wgpu、CPU 对照、前后表面及薄层遮挡、重叠候选、PNG/ID 一致、上传缓存和强制 CPU。
- 共享服务测试覆盖 Frame 捕获、相机/epoch 冲突与迟到请求、旧 revision、预览不提交、压缩平面，以及渲染入口升级/重启保留原历史。
- 真实 Xvfb 桌面和 Chromium 操作 Box/Fillet、预览确认/取消、相机、Undo/Redo、外部修改与断连恢复；Web 另核对收到的帧与 Agent capture 的 PNG 完全相同。
- tests/codex_mcp_host.py 在实际 codex-cli 0.162.1 app-server 中验证 MCP handshake/工具发现；不调用推理 API。
  它不是自动 VLM 性能评测。当前 Codex 会话另实际查看普通 shaded PNG，选择前侧竖边像素后经 MCP pick/Fillet/validate，记录独立保留。
- tests/render_benchmark.py 记录 release Box（12 triangles）和 Fillet（44 triangles），384/640/768，各后端每项三次。
  小场景下 GPU 的 dispatch/readback 成本可能高于 CPU，不能据此承诺 GPU 提速。大模型、移动端和更多显卡需要另行测量。

运行：

```sh
scripts/build-web.sh
cmake --build build -j 6
cargo test --workspace --locked
CAD_PLAYWRIGHT=/path/to/playwright ctest --test-dir build --output-on-failure
python3 tests/codex_mcp_host.py
python3 tests/render_benchmark.py
scripts/publish-web.sh
```

详细结果见 validation.md 和 artifacts/p7、p7-gui、p7-web。仍没有 Persistent Naming、磁盘历史恢复、多 body/特征树或多人协作系统，
也没有 STEP/OCCT 或 Python 产品运行时。


### Web 交互性能修正（2026-10-19）

原 Web 的 wgpu 标签指服务端 compute 渲染，浏览器只有 egui 纹理显示。现在浏览器在自己的 GPU 上对缓存三角网格和边进行 depth-tested rasterization，WebGPU 优先、WebGL2 回退。旋转/平移/缩放逐输入重绘本地相机，不清空等待远程位图。相机静止 150ms 后合并一次服务端视图请求；返回帧仍须满足 epoch、相机、尺寸和文档版本才能用于拾取与确认。原来的共享 Frame/Agent 截图协议保持有效，但交互显示不再被网络帧阻塞。

浏览器回归：`CAD_PLAYWRIGHT=<playwright目录> node tests/web_orbit.cjs`，`CAD_TEST_GPU=webgl` 测回退；可用 `CAD_WEB_URL` 指向本地只读测试网关。测试模拟 48 次触屏移动，阻塞 `/api/view`，验证多张中途截图均有模型且内容变化、无页面异常、WebGPU 确有本地队列提交；记录刷新回调时间，不将其冒称所有设备的实测 GPU FPS。浏览器图形启动参数仅属于测试工具，不要求用户配置浏览器。
