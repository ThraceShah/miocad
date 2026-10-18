# 验证记录

环境：2026-10-15，Linux x86_64，GCC 15.2.0，CMake 4.2.3，Python 3.14.4。
上游固定：c9c5979f04cea8c38213004d4f04df0dcfb40d05。

命令：

```sh
cmake -S . -B build -G Ninja
cmake --build build -j 6
ctest --test-dir build --output-on-failure
./build/kernel_probe
```

结果：独立内核编译成功，CTest 1/1 通过（0.02 秒）。

```text
box: volume=1000 edges=12 valid=true
boolean 0: volume=1500 valid=true
boolean 1: volume=500 valid=true
boolean 2: volume=500 valid=true
fillet: volume=997.854 valid=true source_unchanged=true
tessellation: vertices=16
```

体积核对使用解析期望值与显式容差；Release 下不使用会被 NDEBUG 去除的 assert。
首次编译暴露探针调用 SmTArray::Append 的类型错误，已在自有代码改用 Add，内核源码未变。
探针验证建模与网格能力；完整视觉定位与 GPU 拾取尚未通过验收。

## 首轮服务增量

```sh
cmake --build build -j 6
ctest --test-dir build --output-on-failure
python3 examples/box_fillet.py
```

结果：CTest 3/3 通过（约 0.6 秒）：

- kernel_capabilities：Box、三种 Boolean、Fillet 解析体积、源模型未变、流形/AssertValid、非空网格。
- service_transactions：4 个 unittest，多个文档隔离、单边 Fillet、Undo/Redo、ABA 旧引用拒绝、
  负半径/过大半径/错误尺寸/外来引用/版本冲突失败后状态不变，失败不丢 redo，成功编辑截断 redo，
  原生文件写出与错误扩展名拒绝。
- access_preview_mcp：3 个 unittest，CLI 能力查询，CPU PNG/拾边/过期 render 拒绝，MCP 握手、
  工具发现、Box/Fillet、错误返回和 PNG 图像内容。

示例实际结果：Box volume=1000；Fillet volume=997.8539818196981，faces=7，edges=15，valid=true。
生成 shaded / face-id / edge-id 图，像素拾取后完成圆角并导出 `artifacts/box-fillet.smb`。
已人工查看 Box 和 Fillet 后 shaded 图，圆角可见、投影正常。
生成物不提交 Git，运行示例可重新生成；文档 ID 与 render ID 每次不同。

## 尚未验证 / 已知问题

- GPU picking 未实现；CPU 图像/ID 是预览。边重叠需要进一步消歧。
- 未运行真实 VLM 推理，demo 从可见 edge-ID 像素确定性取样。
- Naming callback 的演化覆盖、跨编辑稳定身份和跨重载引用尚未实现或验证。
- 首轮仅单 body / 单边圆角，Boolean 只在探针层；没有特征树参数重建。
- 只运行默认 AssertValid 级别、流形和体积检查，尚未建立完整几何间隙/公差审计。
- 上游已知 remove_fillet/heal 成功状态不充分、周期 seam 修复、fillet 附近网格方向和某些积分误差；
  详见调查中的官方 KNOWN_ISSUES 链接，本次未修补内核。
- MCP 是最小 stdio 原型；未运行第三方 conformance suite，未验证各真实 Agent host。
- 文档、revision、render 缓存和 Undo/Redo 只在进程内；无崩溃恢复、取消或资源限制。
- 导出 SMB 已验证写出，尚未增加本服务原生 reload/roundtrip 命令。
- Windows/macOS/ARM、完整 Packman/USD 构建未执行。


## 2026-10-17 回滚与范围修订

按用户指示回退到 be22e72 的 SMLib MVP 功能阶段，并移除 STEP 导出和 OCCT 备选内核要求。
本次通过新提交反向撤销 cd749fa、115f88d，保留 Git 历史。
删除对应适配层、构建/依赖配置、测试、MCP/CLI 后端选择和 STEP 命令占位，
清理本地生成的 OCCT SDK、包和示例产物。历史技术调查中的对比事实保留，明确不作为实施要求。

重新配置并构建成功，CTest 3/3 通过；examples/box_fillet.py 运行成功：
Box / 单边 Fillet / Shaded、Face ID、Edge ID / CPU 拾取 / 有效性验证 / 原生 SMB 导出。
STEP 不再属于缺失验收项；当前后续目标以 scope.md 为准。

## P1：C++ Commander（2026-10-18）

先执行既有基线，3/3 CTest 通过。新增直接链接 Commander 的契约测试，先以空实现验证失败，
初始 7 个场景全部按预期失败，再实现库、命令注册表和薄 worker，补充历史响应故障回归。

最终命令：

```sh
cmake --build build -j 6
ctest --test-dir build --output-on-failure
python3 examples/box_fillet.py
```

结果：CTest **4/4 通过**（约 0.63 秒）：

- kernel_capabilities：原有未修改 SMLib 探针。
- commander_contract：8 个原生 C++ 场景，不启动 worker/窗口/网络。
- service_transactions：5 个 Python unittest，增加 JSONL 解析错误、id 回显、命令发现与分类错误。
- access_preview_mcp：原有 3 个接入测试，并验证 MCP 可调用 commands.list。

Commander 场景包括：注册目录完整性；真实 SMLib Box/Fillet/mesh/Undo/Redo；
多文档/多 Commander 实例隔离；错误和参数验证；修改候选后抛异常；无效候选；
候选验证后响应查询失败；Undo/Redo 响应查询失败；原生导出失败清理临时文件并保留原文件。
对应失败均不改变原文档、revision 或 redo 分支。
参数检查包括错误尺寸、负数/浮点/bool/string revision、NaN/Infinity 半径、NUL 文件路径。

示例结果：valid=true，volume=997.8539818196981 mm³，faces=7，edges=15；
继续生成 shaded/face-id/edge-id PNG 和 artifacts/box-fillet.smb。
第三方内核源码未修改。仅新增核心命令目录与 error.kind，保留既有成功响应、错误 code 和原有入口。
尚未实施 P2 Rust/协议生成、P3 共享服务或 GUI；Python 产品入口与 CPU 预览仍待 P4 迁移。

## P2：中性协议与异步 Rust 客户端

基线：P1 的四组 CTest。先新增版本化 wire 验收，旧 worker 在 hello 上四个测试全部失败，
随后实现中性契约、版本端点和 Rust 客户端。过程中的 DTO 可选字段 null 往返问题由共享样例发现并修复。

工具链：Rust/Cargo 1.90.0，固定 rust-toolchain.toml 与 Cargo.lock。
契约来源：protocol/contract.json；C++ 使用 CMake 生成的嵌入头，Rust 使用 include_str!，
不另存手工维护的第二份 schema。Rust DTO 采用测试校核，没有引入代码生成器。

最终验证：

```sh
cmake --build build -j 6
ctest --test-dir build --output-on-failure
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
cargo run -p cad-client --example box_fillet --locked -- build/cad-server
```

CTest **7/7 通过**（约 1.5 秒），Clippy/格式检查通过：

1. kernel_capabilities：原有未修改 SMLib 探针。
2. commander_contract：P1 八个直接契约场景继续通过。
3. service_transactions：旧 JSONL/Python 产品入口回归。
4. access_preview_mcp：原有 MCP/CLI/CPU 预览回归。
5. wire_contract：四个 Python 内部验收，真实 worker 的握手/版本/上下文、Box/Fillet/事件/revision、
   重放不重复执行、请求冲突、状态查询、缓存淘汰/重新 hello 不重置高水位。
6. protocol_schema_contract：C++ 跑 32 个共用样例，核对嵌入契约与磁盘一致并拒绝非有限数。
7. rust_workspace：九个 Rust 集成测试，包含四个协议/类型测试、两个真实 worker 工作流测试、
   三个传输故障/取消/握手高水位测试。

Rust 验收包括全部核心命令的类型覆盖、错误枚举一致、共用样例、有限 f64 位级 JSON 往返，
实际 Box/Fillet/mesh/Undo/Redo/结果查询、非法输入不改文档、请求缓存重放、版本不匹配拒绝，
以及 EOF/超时/畸形响应/关联错误/等待 future 被取消后不自动重发。

数值回归曾以体积 997.8539818196981 复现默认 Rust JSON 解析后相差 1 ULP；
启用 serde_json float_roundtrip 后，包含该值、极大值、极小正常数和 subnormal 的位级测试通过。
这校核传输数值，不宣称内核不同运行间的积分结果在位级完全相同。
Rust 示例实际输出 valid=true、faces=7、edges=15，volume 约 997.853981819698 mm³，revision=2；
示例直接调用 C++，无需 Python 运行时。第三方内核源码未修改。

范围边界：本轮仍为独立 worker 与响应内事件，没有共享 daemon、attach、事件广播或实际重连。
service_id 当前为 null，去重仅限内存中的同一 worker；旧版本 op 格式不提供去重保证。
缓存淘汰后保留高水位并拒绝再执行，结果不确定时必须查状态/核对文档，不保证跨崩溃 exactly-once。
旧 Python MCP/CLI 与 CPU 渲染尚待 P4 替换；未新增 Web/GUI、GPU、STEP 或 OCCT。

## P3：默认共享 Rust 服务（2026-10-18）

先编写共享生命周期验收，再实现 cad-service / SharedClient；第一轮补充测试发现实例身份检查的
断言预期不一致：收到错误 service ID 时，SDK 应拒绝整份响应并报告 identity changed，
不能采用其中的 error 文本。修正测试后通过，身份校验逻辑保留。

验证命令：

```sh
cmake --build build -j 6
ctest --test-dir build --output-on-failure
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
cargo build -p cad-service --locked
cargo run -p cad-client --example shared_session --locked -- target/debug/cad-service build/cad-server /tmp/my-cad-session
# 在第二个独立进程再次执行上一行，然后显式关闭：
cargo run -p cad-client --example shared_session --locked -- target/debug/cad-service build/cad-server /tmp/my-cad-session --shutdown
```

结果：CTest **7/7 通过**（最终约 2.7 秒），其中 Rust workspace **14 个集成测试**；
Clippy `-D warnings`、格式检查、diff 空白检查通过。共享 schema 样例由 32 增至 **40**，C++/Rust 共读。
P1/P2、原 Python MCP/CLI/CPU 预览回归继续通过。

新增四组 daemon 集成测试使用真实 SMLib worker 和专用临时 runtime 目录：

- 启动竞争只附着一个 service ID；共享文档可见；旧 revision 的两个修改只成功一个；重放不多发事件。
  关闭一个客户端后会话仍可用；隔离会话不污染默认文档；超过 64 事件后要求快照重同步。
- 提交请求后丢弃 socket 响应，再 attach 同一身份，request.status 可核对终态，原请求重放不重复创建。
  create_session 幂等 key、显式取消不支持、错误 service ID 和 token 拒绝均验证。
- 真正 SIGKILL worker 后，会话返回 worker_lost/不确定结果，不自动重建；其他隔离 worker 仍可操作。
  显式关闭后旧端点不可连接；残留 endpoint + 已释放文件锁可以恢复新服务。
- 真正 SIGKILL daemon 后，自动启动获得新 service ID 和空文档，旧客户端不能迁移/执行旧请求。
- TCP 代理转发真实提交、故意不回响应，取消 SDK future 后连接逻辑不可继续；重新 attach 查询/重放只保留一次创建。

另以两个独立 SDK 示例进程依次执行真实 Box/Fillet：service ID 相同，第一个进程退出后第二个看见
两个文档，结果 valid=true、volume≈997.853981819698 mm³。机器结果保存在本机
`artifacts/p3-shared-demo.json`（生成物，不入 Git），测试 daemon 已显式关闭。

补充权威枚举：先新增 C++ 文档清单验收，旧 Commander 在目录检查和 document.list 调用上失败；
随后实现中性契约、C++ document.list 和 Rust Command::DocumentList。服务快照直接读取核心完整清单，
不靠提交事件缓存推测文档是否存在。

决策及边界：本机 loopback TCP，一次连接一次请求/响应；外层 service ID/token，内层保留 worker 请求身份。
OS 文件锁在服务终生持有，worker hello 完成后原子发布端点。事件为带游标增量拉取，非推送连接；
快照与 cursor 在同一 worker 锁内生成。每 worker 串行，多个 worker 通过进程隔离 SMLib context。
客户端断连/取消不取消已接受调用；worker 60 秒 watchdog 后终止会话，SDK 15 秒超时可能先发生，需核对结果。
仅 Linux x86_64 已验证，随机源 `/dev/urandom`；无磁盘恢复、跨崩溃 exactly-once 或内核抢占取消。
资源上限和操作说明见 `crates/cad-service/README.md`。未修改第三方内核；未新增 GUI、Web、STEP 或 OCCT。
Rust MCP/CLI 产品入口及 Python 旧入口替换留待 P4。

## P4：Rust 产品入口与服务端场景截图（2026-10-18）

先新增 tests/product_test.py，初始两组验收因 Rust CLI/MCP 程序不存在而失败；随后实现产品路径。
现有 C++ Commander、SMLib、事务与引用语义保留，第三方源码未修改。

新增 cad-access（公共入口适配）、cad-cli、cad-mcp 与 cad-render；CLI/MCP 共用 cad-client，
默认 attach 同一服务。产品目录合并 neutral contract 的 commands/access_commands，并统一收紧未知字段；
MCP inputSchema 从同一目录展开本地 $ref，发布 schema 与执行校验有专门一致性测试。
服务在同一 worker 锁内获取 mesh/edges 和 revision，用 blocking task 渲染/编码 PNG，缓存四帧。
pick 验证核心版本和会话；可见 face/edge ID、PNG 像素及引用使用同一帧映射。

验证命令：

```sh
cmake --build build -j 6
ctest --test-dir build --output-on-failure
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
cargo run -p cad-access --example box_fillet --locked -- --runtime-dir /tmp/miocad-p4-demo-verification
python3 scripts/package.py
CAD_BIN="$PWD/artifacts/dist/bin" CAD_SERVER="$PWD/artifacts/dist/bin/cad-server" python3 tests/product_test.py
```

结果：CTest **9/9 通过**；其中 Rust workspace **17 个集成测试**、产品入口 **5 组端到端测试**。
共用 neutral schema 样例增至 **48**，C++/Rust 同读；Clippy `-D warnings` 与格式检查通过。
P1/P2/P3、Python 内部旧原型回归继续通过。

产品验收使用真实发布/开发二进制，每个产品子进程 PATH 仅含产品 bin，不含 Python/Cargo/CMake：

- CLI 创建 Box，MCP 读取同一文档并请求截图，另一个 CLI 进程使用同一帧拾边、Fillet。
  核对 valid=true、体积约 997.8539818 mm³；过期截图及旧实体引用失败且 revision 不变。
  MCP Undo、CLI Redo、结果截图、MCP SMB 导出均通过，默认文档数量不增加。
- MCP 生命周期、工具发现、2025-11-25 / 2025-06-18 协商、未知版本回退、重复初始化拒绝、
  参数/方法/工具/解析错误、浮点 request ID 拒绝及 initialized 通知校验。
  tools/call 通知不执行建模，通知不产生额外响应；仅协议协商不启动 CAD daemon。
- MCP structuredContent 与 text JSON 一致；截图 ImageContent 与服务 shaded PNG 字节一致。
  标准库测试解析 PNG signature/CRC/尺寸/像素，三类 PNG 均有效。
- 可配置尺寸、可选文件输出与 base64 一致；空文档/越界坐标/错误尺寸拒绝。
  隔离会话不共享文档或帧，四帧缓存淘汰后返回 render_not_found。
- CLI JSONL 在错误后继续，id 回显；--command 失败退出 1，迁移提示脚本退出 2 且 stdout 为空。
  旧启动配置不会悄悄运行另一个独立 Python worker。

Rust 渲染测试逐像素核对 ID 图与 pick 对应、背景、可见面/边、重复候选歧义、坏网格和尺寸限制。
边容差重叠返回 ambiguous/null，不沿用 Python 原型的最后命中覆盖。
服务 renderer 查询及文件输出均不增加几何 revision；修改仍受核心事务/乐观锁保护。

运行真实 Rust 示例时发现 Cargo example 目录比产品 bin 多一层，默认服务路径不存在；已修正默认定位并复验。
实际查看 before/after shaded 与 edge-id PNG：Box 和 Fillet 圆角轮廓正常。
本机 artifacts/p4-demo/ 保留三类 PNG、SMB 与 demo.json；示例选择可见 ID 像素是确定性验收替身，未运行 VLM。
不同 SMLib 运行的积分尾数有轻微差异，体积按既有解析容差校核，不宣称跨运行位级一致。

release 包：artifacts/dist/ 与 artifacts/dist.tar.gz（生成物，不入 Git）。bin 下只有 cad-cli、cad-mcp、
cad-service、C++ cad-server；share 下为文档、锁文件、第三方许可证及依赖清单，没有 .py 文件或 Python runtime。
在包路径上完整重跑五组产品验收通过。另用 env -i（仅 PATH=包 bin）与 strace 跟踪 capabilities → shutdown，
execve 仅出现 cad-cli → cad-service → cad-server，没有解释器或构建工具；记录在 artifacts/p4-runtime-exec.log。
C++ worker ldd 仅需系统 C/C++ 运行库，SMLib 静态链接；仍要求相容 Linux/glibc，不宣称跨发行版通用包。
BUILD_TESTING=OFF 的独立 CMake 配置不查找 Python（禁用查找变量未使用正表明未调用 find_package）。
测试与示例实例均显式关闭，未修改默认用户服务、未推送或发布网络服务。

迁移决定：工具名 cad_command、core op 和结果布局保留；images 改为 base64，显式 directory 的磁盘路径在 files。
原型实现移到 tests/legacy；旧启动路径仅输出迁移提示并退出，不进运行包。
具体运行/配置、资源上限及请求不确定性处理见 docs/migration-p4.md。

范围边界：仅 CPU 场景截图/近似拾取，无窗口截图、egui GUI、GPU、HTTP/WS、任务进度/抢占取消、
Persistent Naming 或磁盘恢复。MCP 已进行真实本地 stdio 测试，尚未逐个 Agent host 安装验证。
STEP 与 OCCT 未恢复。下一阶段为 P5 egui 桌面客户端。

## P5：egui 桌面、只读候选与多入口同步（2026-10-18）

先新增 C++ 预览/场景契约，旧 Commander 在目录与 model.preview 上失败，再实现核心只读能力。
候选复制 Document、复用 Box/Fillet 几何处理器和验证，返回独立临时引用命名空间；正式文档不提交、不截断 redo。
新增 render.scene 在单次核心调用中返回状态/网格/边/历史可用性，避免多次查询间被其他入口修改。

新增 cad-ui-egui（宿主无关界面/读模型/控制器）与 cad-gui（eframe/wgpu、cad-client 后台连接）。
桌面包含文档树、Box/Fillet 参数、可见边选择/高亮、预览/确认/取消、Undo/Redo、状态/错误与原生导出。
相机/选择/悬停/草稿只在窗口保存；业务请求和 CPU 渲染分别在后台线程，旧票据/视图 epoch 不覆盖当前状态。

验证命令：

```sh
cmake -S . -B build -DCAD_ENABLE_GUI_TESTS=ON
cmake --build build -j 6
ctest --test-dir build --output-on-failure
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all --check
python3 scripts/package.py
CAD_BIN="$PWD/artifacts/dist/bin" python3 tests/gui_smoke.py
CAD_BIN="$PWD/artifacts/dist/bin" CAD_SERVER="$PWD/artifacts/dist/bin/cad-server" python3 tests/product_test.py
```

最终范围：CTest **10/10**；Rust workspace **24 个集成测试**，包含四个 UI 控制器、两个真实共享后台、三个 raster 测试。
共享 C++/Rust schema 样例 **54**；P1–P4 和五组 Rust CLI/MCP 产品端到端回归保持通过。
Clippy `-D warnings`、格式/diff 检查通过。默认 GUI 测试开关为 OFF，普通 headless 回归不要求显示；
本阶段明确启用真实桌面验收，不能以仅编译或默认 headless 回归宣称 P5 完成。

控制器/核心/后台覆盖：

- 取消、草稿改变、旧 preview ticket、revision 前进均使候选不可确认；确认被消费一次，不能双提交。
  Scene 查询在快照之后读到更高 revision 时也作废候选；已取消预览的迟到断连仍更新连接状态。
- 候选几何有效但正式文档仍为空/原体积，预览引用不能用于真实 Fillet，预览后 Undo/Redo 不变。
- GUI 后台与独立 SDK 客户端同 service ID，Agent 修改触发场景刷新；旧版本确认收到 revision_conflict。
  后台退出/重开仍读取原文档；服务不可达异步上报，UI 构造不等待服务。
- 本地旋转保持世界包围球尺度与文档版本，像素拾取映射与场景同帧；有界命令/结果队列和视口任务合并。
- 不确定请求保留原 Request，经同实例 status 核对和快照刷新，不自动重放；核对结果不会被普通刷新完成文案覆盖。

真实桌面测试使用独立 Xvfb/XTest，1280×820 原生 eframe/wgpu 窗口；不操作用户桌面。
不是浏览器页面或 GUI mock。实际适配器日志：Vulkan / llvmpipe (LLVM 21.1.8, 256 bits) / Cpu。
依次点击创建文档 → Box 预览 → 取消 → 重新预览/确认，独立 CLI 核对 revision 0→1；重复确认不增加版本。
点击视口可见边并设置默认半径，Fillet 候选不提交，确认后 revision=2，valid=true、volume≈997.8539817 mm³。
实际截图能看到前侧竖边高亮及圆角结果。

继续拖动视口，图像与轴向指示改变，但 revision=2 且事件列表无新增；GUI Undo/Redo 后为 revision 3/4。
MCP 在同一服务读取几何，未显式传 edge 的 Fillet 被拒绝，证明 GUI 选择不被 Agent 隐式消费。
GUI 再次显示候选期间，MCP 替换 Box 为 12×10×10，revision=5；GUI 作废旧预览、禁用确认并显示 stale，体积为 1200。
WM_DELETE_WINDOW 关闭窗口后 CLI/MCP 仍读取原文档；重开保持同 service ID 和 revision=5。
MCP 导出 SMB 后显式关闭测试服务，GUI 显示 Offline/connection refused，提交按钮禁用且缓存场景只读。
已查看 Box/Fillet 预览、圆角结果、旋转、过期候选、重开和服务不可达的真实 PNG。

测试生成 artifacts/p5-gui/ 下 13 张 PNG、native-workflow.smb、acceptance.json 及适配器日志；不入 Git。
GUI 原生验收和 release 包均不包含 Python 产品服务；gui_probe.py / gui_smoke.py 只是内部测试驱动。
已生成五个原生程序的 release 包，收录默认字体许可；GUI 与 CLI/MCP 同包、独立启动。

过程修正：禁用 eframe 默认 feature 后首次原生启动没有实际 wgpu backend，显式启用 Vulkan/GLES 后复验通过。
调整了侧栏滚动、稳定 orbit 中心/尺度和异步状态边界；GUI 错误携带的原 Request 改为 Box，保留恢复信息并通过 Clippy。
打包 metadata 初次尝试下载其他平台源包遇到超时；改为本机 target + offline metadata，仅使用已成功构建的本机依赖。
部分 egui crate 不含 workspace 根许可证，补存官方固定 tag 的许可文本；内核与 egui 源码均未修改。

边界：P5 是 wgpu 显示 CPU raster 的交互场景纹理，不是 GPU 几何渲染/拾取；只验证 Linux/X11/Mesa 软件适配器。
没有测试真实显卡、其他 OS、浏览器传输/发布、磁盘恢复、Persistent Naming、任务进度或内核抢占取消。
未创建/发布 Web 应用，未改用户默认服务，未推送。下一阶段为 P6 WebUI。

P5 release 最终复验：使用 artifacts/dist/bin 中的匹配程序重跑五组产品测试和完整原生窗口验收均通过。
归档约 14.5 MiB，五个程序、无 .py 文件；检查确认默认字体 UFL/OFL 和 supplemental egui 许可均随包提供。
所有测试窗口、Xvfb 和服务已关闭。artifacts/dist.tar.gz.sha256 为最终归档校验值。

## P6 WebUI（2026-10-18）

新增 Rust cad-web：原生 HTTP gateway + WASM egui 宿主。共享 UI/Controller/DTO/renderer，
原生 Bridge 只保留线程/Tokio/SharedClient 调度；SMLib/Commander 代码未修改。

构建和契约：

- Rust 1.90 + wasm32-unknown-unknown、eframe/egui 0.33.3、wasm-bindgen/CLI 0.2.129 实际 release 构建。
- 原生/WASM Clippy -D warnings、cargo fmt --all --check 通过。
- cargo test --workspace：26 个测试通过。网关新增授权到期/页面身份拒绝，以及真实 worker/Agent 事务集成测试。
- 配置 CAD_ENABLE_GUI_TESTS=ON / CAD_ENABLE_WEB_TESTS=ON 后 CTest 11/11 通过（57 秒），
  包含既有内核/协议/CLI/MCP、真实 Xvfb 原生窗口和真实 Chromium canvas 操作。
- 授权测试覆盖无 cookie、错误/缺失/null Origin、跨 session/client、服务器路径导出、内部生命周期信封拒绝，
  限流、注销、到期；两个页面 client ID 不同且共用 worker。Web download 只接受 document，额外 path 字段拒绝。

浏览器测试 tests/web_smoke.cjs，Chromium 141.0.7390.37 / Playwright 1.56.1，1280×820 / DPR=1：

创建文档、Box 候选/取消（revision=0）、确认（revision=1，volume=1000）；点击可见前侧边，
Fillet 预览不修改文档、确认后 revision=2、valid=true、volume≈997.8539818。
旋转改变图像而没有新事件；Undo/Redo 到 revision=3/4。浏览器真实下载 model.smb，未显示/接受服务器路径。
Agent 替换 Box 后候选失效，旧确认不提交；断网后缓存只读，重连拿到 Agent 已修改的 revision=6。
Playwright 在 kernel 完成 model.box 后丢弃 HTTP 回执：UI 显示 outcome_unknown/Offline，权威文档已到 revision=7；
Reconnect 查询原 request.status 后显示已完成，保持 revision=7，无重放。刷新页面恢复同文档，Lock 撤销授权。
已查看 Fillet/旋转、unknown outcome 和核对恢复的实际 PNG；pageerror 列表为空。

上线验证 tests/web_deployment.cjs：

- https://example.ts.net/apps/miocad/、JS、模块 Worker、WASM MIME、health、嵌套路由均 200。
- 真实 HTTPS Chromium 解锁、创建 Box/Fillet；cookie Secure/HttpOnly=true，SameSite=Strict，Path=/apps/miocad/。
- 默认 runtime-dir 与 CLI/Agent 一致；保留一份演示文档 6b7e82d8-1c96dd3f-dbc6c95d-9ff22e0-，revision=2。
- systemctl --user restart miocad-web 后 CAD service ID 和原文档 revision/valid 均未改变；
  原网页授权被撤销，重新解锁恢复场景。第二标签页从嵌套 URL 进入同一工作区。
- portalctl list 收录 miocad 且 online=true；独立 miocad-web / miocad-cad-service 均 enabled/active。
- 门户大文件 base64 正则导致 deploy_service 栈溢出，尚未创建应用 unit；检查日志后改用技能允许的持久本机服务 + proxy，
  未修改门户。其代理统一改写 cookie/Location，因此采用 strip-prefix=true + CAD_WEB_STRIP_PREFIX=1，避免重复前缀。
- 初始测试 Chromium 继承本机系统代理，Tailnet TLS 被代理关闭；仅在该测试 browser context 为 Tailnet 域名设置直连绕过后通过，
  没有修改系统代理、Tailscale 配置或门户。HTTPS 证书正常核验，没有 ignoreHTTPSErrors。

artifacts/p6-web 保存 PNG、acceptance.json、deployment.json、browser-workflow.smb 及网关日志，不入 Git。
打包合并 native/wasm32 依赖许可及字体许可，原生完整包 6 个程序 + WASM，无 .py；Web 包约 27 MiB。

边界：CPU raster/ID picking，WebGL2 仅显示 egui 纹理，GPU 几何路径仍在 P7；复杂模型/移动设备/高 DPR 未验收。
浏览器每授权 8 个页面身份，worker 逻辑 client 总量仍沿用 128 上限；长期开关页面可能耗尽额度，届时拒绝新客户端，
不自动重启服务或丢文档。共享 origin 的其他门户应用属于同一信任边界；没有新增公网多租户账号系统。
CAD daemon 自身重启仍失去内存文档；Web 重启保留文档。内核没有导入文档接口，因此没有伪上传入口，也无 STEP/OCCT。

本阶段只本地 Git 提交，不推送；保留生产 Web/CAD 两个持久服务，关闭开发验收服务/窗口。

## P7 共享 GPU 视口（2026-10-19）

实现：cad-render/wgpu 27.0.1 的 surface/depth/Face ID 和 edge/depth-test 两遍 compute；
桌面/Web/Agent 同用服务 Frame，新增 render.frames/capture。CPU fallback、版本/相机/视口序号检查、
候选列表、薄层遮挡、压缩平面和缓存均有运行验证。没有修改 SMLib 或第三方内核源码，没有加入 Python 产品服务。

测试：

- cargo test --workspace：31 个 Rust 测试通过，包括新增 GPU、共享视口/relay、UI 迟到帧和压缩平面错误测试。
- CTest 12/12，通过内核、Commander、事务、协议 54 个共用样例、Rust、CLI/MCP、真实桌面、Chromium 和实际 Agent host。
- 原生/WASM Clippy -D warnings、fmt、脚本语法及 Git diff 检查通过。
- GPU 测试核对 PNG/ID、遮挡/薄层、歧义 null+候选、精确字节上传缓存、强制 CPU 和严格 GPU 错误。
- 服务测试核对同 render ID 原始捕获、视口相机/epoch 冲突及乱序、过期 revision、预览不提交、CPU 深度输出；
  relay 经过两次启动/停止，原 worker/service identity、Box 文档和 Undo 历史保持。
- UI 测试拒绝旧 epoch、不同相机/尺寸/revision 的 Frame；确认按钮只在候选帧已到达后启用。
- 真实 native Xvfb 窗口用 Vulkan/llvmpipe，完整 Box/Fillet/Undo/Redo/外部修改/关闭重开/断线通过。
- 真实 Chromium 用服务端 Radeon 780M 的 wgpu 帧，完整流程及丢回执核对通过；
  浏览器收到的 Frame.images.shaded 与 Agent render.capture 完全相同，GPU ID 像素引用与实际点选闭环一致。
- MCP render.capture 与 render.snapshot 均提供真正 image/png 内容，结构化图片与 image content 字节一致。

实际视觉验证（与确定性测试分开）：当前 Codex 会话只查看 384×384 普通 shaded PNG，
定位前侧竖边 (192,245)，MCP render.pick 返回唯一 edge 引用，Fillet radius=1 后 revision=2、valid=true、
volume=997.8539816854025，并获取结果截图。没有通过搜索 ID 图来决定该像素。
实际适配器为 AMD Radeon 780M Graphics / RADV PHOENIX / Vulkan / IntegratedGpu。
这是一个实际视觉闭环样例，不是通用 VLM 准确率或稳定性统计。artifacts/p7/visual-selection.json 及前后 PNG 保留记录。

tests/codex_mcp_host.py 使用真实 codex-cli 0.162.1 app-server，临时运行参数配置本项目 MCP，
验证初始化、工具发现和 cad_command schema；没有修改用户配置，也没有发起模型推理 turn。
实际 host 原生工具发现与当前 Agent 的视觉/MCP bridge 调用分别说明，未冒称多种 Agent 的全面兼容性认证。
配置方式参考 [官方 app-server 文档](https://developers.openai.com/siwc/token-sharing-open-source/codex-app-server)。

性能：release，Box 12 triangles / Fillet 44 triangles，384/640/768，每项三次；含 CLI 进程和 PNG 返回的往返中位数：

| 模型 | 尺寸 | wgpu ms | CPU ms |
|---|---:|---:|---:|
| Box | 384 | 21.13 | 14.31 |
| Box | 640 | 45.83 | 31.09 |
| Box | 768 | 64.04 | 37.13 |
| Fillet | 384 | 29.51 | 16.62 |
| Fillet | 640 | 61.60 | 31.62 |
| Fillet | 768 | 86.55 | 43.37 |

这些小场景 GPU 不比 CPU 快；不能推断大模型性能。缓存命中、adapter、各次 render/往返耗时及图片字节数在 performance.json。
服务 Frame 的 RGB/ID 改为有界 zlib/base64，384×320 测试帧整包小于 1 MiB；
render.frames 不再回传重复 PNG，避免简单列举触发数 MiB 输出。相机改变仍重投影/上传，没有夸大缓存覆盖范围。

上线：https://example.ts.net/apps/miocad/，门户 online=true。
升级前后原 CAD owner PID=673288、service ID、原文档/版本/事件 cursor 完全相同；没有为升级重启模型拥有者。
独立 miocad-render-service 接管发现入口，只更换监听地址，原 worker 和 dedup/high-water 仍由旧拥有者管理；
SDK 显式重连可跟随同身份的新监听地址，拒绝新实例。
实际 HTTPS Chromium 新建 Box/Fillet、同帧捕获、Secure/HttpOnly/SameSite cookie、资源/WASM/嵌套路径和门户通过。
重启 render relay 与 Web 后原模型 revision/valid 保留，重新解锁恢复；三项 user systemd 服务 enabled/active。
生产 Web 要求 relay 已发布，避免开机次序导致误附着旧入口。

过程修正：原始固定截图等待不足以覆盖远程 GPU 帧到达，测试改为等待实际帧，UI 也不再用旧图冒充新候选。
编译 C++ worker 时不再同时运行依赖它的测试，避免链接替换过程的短暂启动失败；中性 schema 修改后重建嵌入契约。
先前 RenderFrames 元数据摘要与 schema 不符的问题已由真实 UI/服务测试发现并修正。

边界：三角化的 GPU/CPU 视图拾取，浮点/栅格边界有数值容差，不是精确 B-Rep 射线求交；
GPU 候选最多三个，溢出明示；Frame 缓存四份、视口 128、逻辑客户端仍有原 128 配额。
服务端集中渲染需要网络往返，Web 没有直接 WebGPU 几何路径；大网格/更多设备/OS 仍须测量。
没有 Persistent Naming、磁盘历史恢复、多 body/特征树或多人协作系统，原文档拥有者崩溃仍丢失内存文档。
所有改动本地 Git 提交，不推送；只保留生产持久服务，测试窗口/临时服务关闭。
