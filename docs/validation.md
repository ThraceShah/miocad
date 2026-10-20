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

## 2026-10-19：Tailscale WebUI 免访问码

当前发布脚本显式设置 `CAD_WEB_TRUSTED_ACCESS=1`；其余启动默认仍要求访问码。共享 `/api/access` 模式查询与登录签发保留来源检查，可信部署自动建立原有范围的浏览器授权，隐藏输入页与 Lock 按钮。原访问码文件保留，可通过移除变量并重启 Web 网关恢复。

验证通过：
- Rust 网关集成测试：默认空码拒绝、正确码登录、免码模式登录、缺失/恶意 Origin 拒绝、无 Cookie 请求拒绝、独立客户端、Cookie 属性和注销撤销；既有事务/回滚/预览/版本冲突用例继续通过。
- `cargo clippy -p cad-web --all-targets --locked -- -D warnings`、`cargo fmt --all --check`、JS 语法与 diff 检查。
- `CAD_BIN=target/release node tests/web_smoke.cjs`：默认访问码模式真实浏览器建模、预览、Fillet、共享视图、失联恢复和锁定回归通过（独立临时文档服务）。
- `node tests/web_trusted.cjs`：实际 Tailscale HTTPS 浏览器自动进入、刷新、嵌套路径、资源、WASM、wgpu 帧、Secure/HttpOnly/SameSite Cookie 和跨源拒绝通过，无页面异常；全程未输入访问码或修改线上文档。
- 部署前后共享文档快照完全相同：两个文档均 revision=2 / valid=true，事件 cursor=6；worker/session 身份不变；原文档拥有者 PID 仍为 673288。
- 三项 systemd 服务 active；门户 HTTPS 200，miocad online=true。证据保存在 `artifacts/trusted-web/`（Git 忽略），含 workspace.png、browser.json 和前后快照。

## 2026-10-19：Web 触屏旋转不再等待远程位图

定位：旧 Web 使用服务端 wgpu compute 的位图；每次输入改变 view_epoch 后立即隐藏纹理，连续手势返回的帧又因相机不一致被丢弃，造成白底与 Updating viewport。此处标签并不代表浏览器在执行 WebGPU 三维 raster。

实现：Web eframe 切换 wgpu，优先 WebGPU、无可用 WebGPU 适配器时回退 WebGL2。共享 local_view 模块用 depth-tested 三角面/边管线绘制同一份 Scene，按 Arc 场景身份缓存 GPU 网格，相机只更新 48 字节 uniform。手势显示不依赖服务端 Frame；相机静止 150ms 后请求一帧用于原有拾取/截图/确认校验，不放松相机、epoch、revision 检查。静止手指的零位移不再推进 epoch。

验证通过：
- `cargo test --workspace --locked` 全部通过；含新增相机 pivot/depth 范围验证和既有预览、版本冲突、帧过期拒绝、共享服务事务测试。Cargo 报原有两份 box_fillet 示例同名输出警告，不影响测试。
- native 和 wasm32 的相关 Clippy（`-D warnings`）、fmt、JS 语法、diff 检查通过。
- 默认访问码模式完整浏览器建模回归通过：Box/Fillet 预览、确认、共享截图、恢复与锁定，测试使用独立临时服务。
- 新增 `tests/web_orbit.cjs`：真实 HTTPS 模拟 48 次触屏移动，故意阻塞 `/api/view`；所有中途采样均有模型，五种不同图像，未收到远程新帧也能旋转，无页面异常。WebGPU / WebGL2 路径都通过。
- 实际 HTTPS WebGPU：约 2994ms 内客户端 GPU 队列提交 149 次；刷新回调间隔中位数 / P95 均约 16.7ms。WebGL2 刷新回调间隔中位数约 16.7ms、P95 约 16.8ms。它们是本测试环境数据，回调间隔不是跨设备 GPU FPS 保证。手势期间至停下后的远程视图请求仅一次，且被测试阻塞。
- `tests/web_trusted.cjs` 实际 HTTPS 自动进入、刷新、嵌套路径、资源/WASM、会话 Cookie 回归通过。
- 发布前后生产快照完全相同：两个文档 revision 分别 3、2，均 valid=true，cursor=7，worker/session 身份未变，原文档拥有者 PID 仍为 673288。只更新 Web 和独立渲染 relay，三项持久服务 active；门户 HTTPS 200，miocad online=true。

证据：`artifacts/orbit-https/`、`artifacts/orbit-https-webgl/`、`artifacts/orbit-https-auth/` 及 `artifacts/orbit/{before,after}-deploy.json`（均 Git 忽略）。本地测试网关和临时 Xvfb 已关闭。

测试环境问题：Chromium 141 在原 SwiftShader/ANGLE 参数下连独立 WebGPU 清屏程序也会丢失设备并报告 external Instance。WebGPU 测试改用测试进程自己的 Vulkan 图形参数后通过；WebGL2 软件回退也单独验证。没有修改系统浏览器、全局网络、第三方源码，亦没有加入未经证实的应用实例生命周期补丁。客户端显示与服务端截图依照同一场景/投影，但 raster 抗锯齿及边线宽度不保证逐像素相同；精确 B-Rep 和拓扑引用仍由服务端内核负责。

## 2026-10-19：触屏旋转、边命中容差和明显高亮

问题确认：原水平 yaw -= dx 令前表面逆着手指移动；拾取仅检查单像素且依赖 hover，触摸松手/刚旋转后匹配 ID 帧尚未返回时可能静默漏选；客户端 GPU 选中线仍只有一像素。

修改：抓取式转台随手指方向、按视口归一化灵敏度；鼠标 6 / 触屏 14 CSS px 椭圆命中范围，选择最近可见 ID，交点近似等距保持歧义，遮挡边不透选。点击使用交互位置并可排队等候新帧，3 秒超时及 epoch / 文档 / revision / 连接 / 预览变化均使其失效。边线改为有深度测试的屏幕空间三角带，悬停青色、选中加粗琥珀色，选中状态文本同色；服务端精确模型/事务协议不变。

验证：
- `cargo test --workspace --locked`：36 项通过；新增抓取方向、视口尺寸一致性、命中容差、最近边/歧义/遮挡、队列单次解析及相机/版本/超时失效测试。
- native / wasm32 相关 Clippy (`-D warnings`)、fmt、JS 语法、diff 检查通过。
- CTest 新增 `web_touch_and_picking`，复用 `rust_products` fixture，在独立临时 SMLib 服务中创建 Box；无需读取生产 runtime。`ctest -R '^web_(workspace|touch_and_picking)$'`：构建 fixture、既有完整建模闭环、新触屏拾取测试 3/3 通过，总耗时约 113 秒。
- WebGL2 DPR 1 / DPR 2、WebGPU DPR 1 真浏览器通过：九条可见边在偏离线条 10 CSS px 的触摸位置均选中，并逐条用实际 Fillet 预览请求核对 Edge 引用。悬停青色与选中琥珀色以 PNG 像素核验，选中线有数百至千余可见琥珀像素；DPR 2 / WebGPU 另测旋转后的三条边及新 ID 帧被阻塞时排队点击的解析。所有预览、相机、选择操作后文档快照不变，无页面异常。
- 为使高分屏回归真实可靠，测试 Chromium 同时设置原生 `--force-device-scale-factor` 和上下文 DPR；仅改上下文 DPR 会使测试版 Chromium ResizeObserver 的物理尺寸与 JS DPR 不一致，已由真实截图发现。没有修改产品或第三方代码来绕过此测试环境差异。
- 最终 HTTPS 部署后重新跑 `web_orbit.cjs`：48 次触屏移动，远程视图返回被阻塞时所有采样仍有模型、五种不同图像，约 3 秒内 149 次客户端 GPU 提交，刷新回调中位数 / P95 约 16.7ms，无异常。`web_trusted.cjs` 免码进入、刷新、嵌套路径、资源/WASM 和 Cookie 回归通过。
- 发布前后两个生产文档仍 revision 3 / 2 且 valid=true，cursor=7，worker/session 和 owner PID=673288 保持不变；三项 systemd 服务 active，门户 HTTPS 200、miocad online=true。

固化源码：`tests/web_interaction.cjs`、`crates/cad-render/src/interaction_tests.rs`、widgets 内 pending-pick 回归及 CMake 测试登记。运行说明见 `docs/viewport-p7.md`。证据位于 Git 忽略的 `artifacts/interaction-*`、`artifacts/interaction-rust-tests.log`、CTest 日志和 `artifacts/interaction-deployment/`（含部署前后快照）。临时测试服务已关闭。

边界：命中容差并不消除交汇或极细、互相接近的边的歧义，此时提示沿目标边移动；仍按当前可见网格 ID 拾取，不是精确 B-Rep 射线求交。测试覆盖 Box 的所有九条可见边和旋转后的代表边，未声称任意复杂曲面、遮挡情况或所有实体触屏设备已全面认证。代码本地提交，不推送。

## 2026-10-19：Document 1 微小倒圆的重合边无法选择

只读调查生产 Document 1（revision=5，faces=10 / edges=24）确认：三个明显倒圆外，还存在半径 0.001mm 的微小倒圆。该角的两条长切边分别位于 (10,0.001,z) 与 (9.999,0,z)，均长 10mm，引用尾号 edge:7 和 edge:9。在当前缩放下两者小于一个像素，整个中段始终得到相同距离的候选；旧 UI 仅提示换位置，缺少真正的消歧路径。没有缺失拓扑，没有修改 SMLib 或放宽遮挡深度阈值。

修复：在最近候选不可分辨时打开显式选择窗口，过滤距离明显更远的候选，显示拓扑编号/长度。支持大触屏按钮、数字快捷键、青色悬停、选中琥珀色、Esc/Cancel；候选按文档/revision/相机 epoch/连接/预览/匹配帧绑定，状态变化时丢弃，拒绝列表外引用。只选择现有边，不自动撤销微小倒圆或改变模型。

验证通过：
- Rust workspace 38 项通过。新增独立 SMLib 服务生成的固定样本，CPU/GPU 在 460×452、长边 z=2/5/8 处均同时保留两个微小倒圆切边候选；单元测试验证逐个显式选择、拒绝列表外引用及相机变化后旧候选不可用。
- native / wasm32 相关 Clippy (`-D warnings`)、fmt、JS 语法、diff 检查通过。
- 新增 `tests/web_micro_fillet.cjs` 和 CTest `web_micro_fillet_picking`：独立临时服务创建三个普通倒圆 + 0.001mm 微小倒圆，真实浏览器触屏按钮选择第一条、数字键选择第二条，核验两个不同引用和可见琥珀像素，移动相机使旧选择器失效，模型快照不变。WebGL2 DPR 1 / DPR 2、WebGPU DPR 1 通过。
- CTest 构建 fixture、新微小倒圆回归及原 `web_touch_and_picking` 3/3 通过，总耗时约 82 秒；原九条可见边、旋转后的三条边、排队拾取及高亮继续通过。
- 发布后在真实 HTTPS 的原始 Document 1 上直接验证，而非仅测试新建 Box：有 558 个像素同时保留 edge:7 / edge:9，候选窗口分别选中二者（界面 Edge 8 / Edge 10），仅发出 `render.scene` 查询，没有建模请求。截图与引用记录于 `artifacts/unpickable-edge/live-*`。
- 生产状态前后完全相同：文档 revision 5 / 2、valid=true、cursor=9，原 owner PID=673288 未重启。三项持久服务 active，门户 HTTPS 200、miocad online=true；`web_trusted.cjs` 免码访问、刷新、嵌套路由和资源回归通过。

固定几何样本由独立测试服务生成，不含生产文档数据；生成配方及边界见 `docs/viewport-p7.md`。证据、只读调查数据和模型前后快照保存在 Git 忽略的 artifacts 中。此修复提供对现有两条切边的明确选择，不代表恢复原始直角边，也没有执行任何 Undo / Redo / Fillet 提交。所有更改本地 Git 提交，不推送。

## 2026-10-19：竖屏 CAD 工作区和双指缩放

按 SolidWorks 式布局组织共享 egui 工作区：文档标签、视图上方矢量图标工具栏、按需左侧 PropertyManager、视口内半透明 FeatureManager。默认浏览不保留侧栏。树展示真实共享操作记录和拓扑，暂不支持回编历史特征参数。修正竖屏远程帧请求的纵横比；新增双指围绕中心缩放、双指平移、Zoom ± 和 Fit，并抑制多指结束后的误旋转/拾取。详情见 `docs/tablet-ui.md`。

验证通过：
- Rust workspace 40 项测试通过；新增相机缩放锚点、平移、Fit 和 768×1024 / 820×1180 / 1280×820 布局测试。native / wasm32 Clippy（`-D warnings`）、fmt、JS 语法、Python 编译及 diff 检查通过。
- 新增独立服务浏览器测试 `tests/web_tablet.cjs`，固化到 CTest `web_tablet_workspace`。CTest 含构建 fixture 2/2 通过（23.16 秒）；WebGL2 DPR 2 / WebGPU DPR 1 均通过。覆盖双指放大/缩小/平移、方向不变、不误选、Fit、图标缩放、文档标签、左右布局、横竖屏比例、真实输入尺寸14的预览/取消，前后文档快照完全相同。
- 原完整 Web 建模、断线/丢失确认核对、九条可见边及旋转后拾取、微小倒圆双边消歧、桌面 Xvfb 建模回归均迁移到真实控件边界并通过。桌面和 Web 使用相同本地 depth-tested GPU 视口。
- 发布后真实 HTTPS 的 768×1024 / DPR 2 竖屏检查通过：全宽视口、双指放大、Fit、左侧参数、标签切换，无页面异常，RPC 仅 `render.scene`，未提交建模命令。人工检查竖屏与参数面板截图。
- HTTPS WebGPU 旋转回归通过：48 次触摸移动，约3022ms内149次客户端 GPU 提交；阻塞新远程视图时所有采样仍有模型、五种不同图像，刷新回调中位数/P95约16.7ms。此为测试机器数据，不代表所有平板硬件帧率。免访问码、刷新、嵌套路径、WASM资源和 Cookie 回归通过。
- 发布前后生产快照完全相同，文档 revision 5 / 2、cursor=9，原 owner PID=673288 未重启；三项持久服务 active，门户 HTTPS 200，miocad online=true。本地只读测试网关已关闭。

证据：`artifacts/tablet-layout/`、`artifacts/tablet-webgl-2/`、`artifacts/tablet-webgpu-1/` 及相应原有建模测试目录（Git 忽略）。使用 Chromium 模拟触屏与实际 GPU 路径测试，尚未连接用户实体平板做硬件认证。未修改第三方内核源码、建模协议或生产几何。本地 Git 提交，不推送。

## 2026-10-19：FeatureManager 与视口等高

移除树滚动区 42% / 320px 的高度限制。透明叠加面板与视口上下对齐，扣除边框、标题与实体摘要后，剩余高度全部用于内部滚动；不挤占视口宽度。折叠树内容、顶部 Tree 隐藏功能保留。

原布局单元测试新增面板顶端与高度断言（768×1024、820×1180、1280×820）；浏览器平板测试新增竖屏、打开参数栏和横屏后的等高断言。单元测试、Clippy、fmt、语法及 diff 检查通过；CTest `web_tablet_workspace` 含构建 fixture 2/2 通过（23.13秒），双指缩放/平移等原检查继续通过。实际 HTTPS 768×1024 / DPR2 等高及参数面板检查通过，无页面异常、RPC 只有 render.scene；截图位于 `artifacts/full-height-tree/`。生产文档快照前后完全相同，owner PID=673288 未重启，Web/relay active，门户 miocad online=true。没有修改第三方或建模逻辑。

## 2026-10-19：带命名引用的本机 CAD Agent 对话闭环

新增 Rust `cad-agent`，统一 Turn/Reference/Attachment/Reply/Proposal 协议和本机 Pi / json-stdio 薄适配。共享 egui 底部对话面板支持视图 Edge/Face/Body、树 Box/Fillet 操作记录引用、名称 chip、文本/PNG/JPEG附件、关联树/视图高亮；中文字体及 Apache-2.0 声明随产品发布。宿主补充真实场景摘要，校验文档/revision/拓扑/事件，并在推理后和预览前复核；推理只产生建议，预览/Confirm 复用原 Commander 事务链路。完整配置与功能边界见 `docs/agent-workspace.md`。

通过的验证：
- Rust workspace 45 项通过，包含伪造拓扑、跨文档/旧版本、未知操作和额外参数、附件类型、延迟回复 ID / revision、过期高亮检查；native all-target 和 wasm32 lib Clippy（`-D warnings`）、fmt、JS语法、Python编译及 diff 检查通过。桌面 release 构建成功。
- CTest 原桌面建模、平板缩放/平移、完整 Web 建模/丢失确认核对、微小倒圆消歧，含构建 fixture 5/5 通过（79.49秒）。
- 新增并登记 `web_agent_references`，fixture + 测试 2/2 通过（19.43秒）；最终脚本直接运行也通过。真实 Chromium/WASM → Rust Gateway → 确定性进程适配 → 真实 SMLib 预览/确认，核验四类引用、准确边 target、文本/PNG字节、Box建议预览/取消、无效/旧建议拒绝、推理中实际并发修改、跨源拒绝。竖屏768×1024与横屏1280×1000布局通过。截图检测到1370个紫色边像素和102720个金色表面像素，关联颜色确实进入实际 GPU 画面。
- `CAD_AGENT_LIVE=1` 调用本机已登录 Pi 做真实推理：四类引用及附件 → 指定边1mm倒圆建议 → 内核预览 → Confirm 后 valid=true；推理和预览期间快照不变。WebGL2路径通过，WebGPU路径另验证整条链路，并再次询问1到2mm变半径倒圆：Agent明确说明不支持，proposal=null，revision保持不变。真实推理未替代常规离线回归的确定性样本。
- 新增 `desktop_agent_references`：隔离 Xvfb 的真实 egui 窗口从树引用边，输入对话，走原生 Bridge/进程适配得到准确建议，预览不提交，确认后revision=2且模型有效，旧引用停止高亮。测试驱动器每次清空上一次探针数据并显式设置X11键盘焦点。
- 发布后的真实 Tailscale HTTPS / WebGPU / 竖屏页面引用生产文档 Body 1 和 Box 1；本机 Pi正确识别两者、说明当前能力，并按请求返回proposal=null。浏览器只发出render.scene查询，没有建模请求；页面无异常，资源/WASM/health可访问。免码、Cookie、刷新与嵌套路由回归通过。
- 发布前后生产快照完全相同：两个文档revision=5/4、cursor=11；owner PID=673288未重启，三项持久服务active，门户HTTPS200、miocad online=true。测试建模始终使用独立临时服务，临时窗口/网关已关闭。只在本地Git提交，不推送。

证据：`artifacts/agent-chat/`、`artifacts/agent-chat-live/`、`artifacts/agent-deployment/` 和CTest日志（Git忽略）。真实生产验收仅查询/客户端选择/对话，不修改生产零件。

当前边界：只执行Box替换和单边定半径Fillet。特征引用指向不可变操作记录，视图高亮是当前实体上下文，未声称具备历史特征到生成面的演化映射。尚无变半径倒圆、凸台、抽壳等建模接口；聊天和附件暂不持久化；不解析任意二进制CAD/PDF附件。当前Agent适配通过禁用Pi工具及类型校验约束建议，不是操作系统进程沙箱。所有限制在界面或使用文档中明确。

## 2026-10-19：Agent 默认自主执行、多步反馈与整组撤销

上一节的“建议 → 用户预览 → 用户确认”版本已替换。新默认：用户发送目标后，Rust 宿主启动后台任务，Agent 持续读取真实场景、发出下一步操作、直接调用 Commander 提交并验证 B-Rep，再根据已执行回执和内核错误继续推理，直到完成。支持32步默认操作预算（宿主可调1–128）、内核拒绝反馈修正和失败时逆序回滚。界面没有 Agent 预览/审批按钮；完成后可 Undo Agent task 撤销整个任务，手动建模 UI 保持原流程。

后台任务有稳定任务 ID、私有写前日志、单独协议客户端和状态查询；同一任务重复 start 不重复建模，刷新页面查询原任务而非重发命令。断开 HTTP 不取消后台工作。整组 Undo 同样在后台执行，重复 Undo 不再修改；revision 检查阻止覆盖其他客户端的后续编辑。无法确认写入结果时只查询原 request.status，保留 pending 回执并停止，禁止盲目重放。Agent 返回的文本不能改变执行器对“不确定结果”的判定。多步任务由多个内核事务组成；若中间有外部写入阻止回滚，会明确报告 partial，保留别人的编辑。宿主重启中断任务，保留日志、禁止自动继续；跨网关授权重建的任务自动接续尚未实现。

验证：
- Rust workspace 47项通过；native all-target / wasm32 lib Clippy（-D warnings）、fmt、JS语法、Python编译、release构建及diff检查通过。
- 更新 web_agent_references 和 desktop_agent_references，直接 Send 后校验已提交有效实体，整个测试不点击 preview / confirm。浏览器覆盖精确引用边自动倒圆、多步 Box→读取新几何→选择新 revision 的边倒圆、整个任务 Undo、已执行一步后失败的整组回滚、实际内核拒绝大半径后反馈修正、丢失启动响应后的幂等查询、刷新期间后台完成、重复 start 不重复写入、保护后续编辑及跨源拒绝。桌面实际 Xvfb 窗口验证自动提交和任务 Undo；进度消息不会因窗口未及时消费而阻塞后台建模。
- CTest 桌面Agent、浏览器Agent、平板手势、原完整Web建模、微小倒圆消歧，含构建fixture 6/6通过（90.12秒）；随后加入内核反馈修正、异步整组Undo和回执保护的定向回归通过，fixture+Agent测试2/2通过（30.32秒）。接入兼容修复后Rust产品/MCP/CLI访问、桌面Agent、浏览器Agent含fixture 4/4通过（37.46秒）。
- 真实已登录 Pi / WebGPU：引用边直接完成1mm倒圆，无审批，随后任务Undo恢复模型；变半径请求不执行替代操作；明确要求替换为12×10×8 Box再自主选择新边做1mm倒圆，Agent实际连续执行两步并报告完成，整组Undo恢复任务前模型。全部在独立临时服务建模，无页面异常。

发布验收发现旧文档拥有者已经达到128个协议客户端上限，导致新浏览器无法进入。未重启原拥有者、未清空文档、未修改第三方内核。Rust service/relay 增加逻辑客户端兼容层，通过已有保留内核通道发送请求并持久保存逻辑请求高水位、原请求/内核请求映射及结果；写前预约、冲突拒绝、过期结果不重放和 request.status 均保留。单元测试真实 C++ worker 连续140个客户端成功、逻辑回执相关性、模拟丢失内核ACK与传输层重载后只创建一个文档、冲突拒绝及状态查询通过；原服务生命周期、丢失回执、relay重启保留文档测试继续通过。旧拥有者汇总事件的来源 client 可能显示保留通道，内联回执恢复逻辑身份；任务权限与撤销不依据该来源字段。

实际HTTPS竖屏验收：新页面免码进入，命名实体/特征引用、高亮、资源/API/嵌套路径通过；确认无Agent预览/确认控件。使用过期revision任务验证后台start/status及执行前拒绝，steps=0，未向生产模型执行建模；真实推理与自动提交已在隔离服务验证。47项Rust测试、真实性推理、截图和回执证据在 `artifacts/agent-chat/`、`artifacts/agent-chat-live/`、`artifacts/agent-autonomy-deployment/`（Git忽略）。

最终发布后再次通过HTTPS免码/刷新/嵌套路径回归、新浏览器和新CLI逻辑客户端连接、后台任务过期scope拒绝（steps=0）。生产快照与本次发布前完全一致：revision=12/4、cursor=18、worker/session身份不变，owner PID=673288未重启；三项持久服务active，门户HTTPS200、miocad online=true。未向生产文档执行测试建模，所有自动提交/回滚/Undo真实性测试均使用独立临时服务。

## 2026-10-20：参数特征、独立编辑历史及原子任务

实现与边界见 [parametric-history.md](parametric-history.md)。完整 CTest 21/21、Rust 48项、独立兼容升级2/2、原生及WASM Clippy、实际WebGPU竖屏特征编辑通过。HTTPS已发布，Document03为新框架示例；原有两份文档、worker/session和拥有进程保持不变。测试涵盖上游修改重算、缓存不重放、失败保留redo、原子任务/冲突、磁盘故障、重启恢复及缺文件拒绝启动。
