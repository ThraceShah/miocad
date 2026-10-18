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
