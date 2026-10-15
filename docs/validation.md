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
STEP 不属于本探针支持范围；不能声明完整八步工作流已通过。

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
  原生文件写出，STEP/错误扩展名不产生假文件。
- access_preview_mcp：3 个 unittest，CLI 能力查询，CPU PNG/拾边/过期 render 拒绝，MCP 握手、
  工具发现、Box/Fillet、错误返回和 PNG 图像内容。

示例实际结果：Box volume=1000；Fillet volume=997.8539818196981，faces=7，edges=15，valid=true。
生成 shaded / face-id / edge-id 图，像素拾取后完成圆角并导出 `artifacts/box-fillet.smb`。
已人工查看 Box 和 Fillet 后 shaded 图，圆角可见、投影正常。
生成物不提交 Git，运行示例可重新生成；文档 ID 与 render ID 每次不同。

## 尚未验证 / 已知问题

- STEP import/export 未实现，因此完整第三阶段八步闭环没有通过；不能将 SMB 导出作为替代验收。
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
- Windows/macOS/ARM、完整 Packman/USD 构建、OCCT 实测比较未执行。

## 第二轮：STEP 原生服务（2026-10-16）

新增可选 OCCT 7.9.2 adapter（Ubuntu 包 7.9.2+dfsg-4）。
先新增 4 个 STEP 验收测试，在旧服务上全部失败；实现后通过。
继续增加 periodic/NURBS 和 unsupported-components，现有 CTest 4/4 通过，STEP suite 共 6 个 unittest。

- Box 与 Fillet：STEP 文件头、MANIFOLD_SOLID_BREP、无 TRIANGULATED_FACE_SET；往返体积相对容差
  1e-7（绝对下限 1e-6），bounds 绝对容差 1e-5 mm，faces/edges 一致、valid=true。
- Cylinder / Sphere / Torus / NURBS cylinder：解析体积、非空网格、边查询、再次 export/import。
- 米制 STEP 输入换算为毫米；2×3×4 m → 2000×3000×4000 mm，体积 24e9 mm³。
- corrupt / empty / missing STEP、stale revision 均不改变模型，也不破坏 redo。
- 成功导入截断 redo；旧引用拒绝，新引用仍能进行 Fillet。
- 多 solid、开放 sheet、solid+sheet 混合输入均拒绝，原文档保持不变。
- NURBS 积分回归曾失败；改用带 knot span 的 Gauss-Kronrod 积分后以原容差通过。

构建说明、依赖与决策见 ADR-002。上文“STEP 未实现”是第一轮历史记录；现在 OCCT 后端支持，
SMLib 后端仍不支持。此段记录仅说明原生服务测试，CLI/MCP 完整工作流验证在后续记录中。

## 第二轮：CLI / MCP / 图像闭环交付

最终双后端配置：`CAD_ENABLE_OCCT=ON`，CTest **6/6 通过**（约 2.2 秒）：

1. kernel_capabilities（未修改的 SMLib 原生探针）
2. service_transactions（SMLib）
3. access_preview_mcp（SMLib；STEP 工作流测试按能力跳过）
4. step_roundtrip（OCCT，6 个 STEP 验收 unittest）
5. service_transactions_occt
6. access_preview_mcp_occt（CLI/MCP 都实际写出 STEP 并导入新文档；图像像素拾边示例同测）

额外验证 `CAD_ENABLE_OCCT=OFF` 后重新编译成功，3/3 CTest 通过，再恢复 ON。
SDK bootstrap 脚本已实际运行成功；固定 OCCT 包版本并产生 packages.json。
第三方 SMLib 工作区无修改；无远端推送。

实际运行：

```sh
python3 examples/box_fillet.py --kernel occt
python3 examples/box_fillet.py --kernel smlib
```

OCCT 示例结果：

- 结果模型 volume=997.8539816339742 mm³，faces=7，edges=15，valid=true。
- STEP 导入新文档 volume=997.8539816339755 mm³，faces=7，edges=15，valid=true。
- 两者 bounds 在 1e-5 mm 以内一致（最大点均为 [10,10,10]）。
- 生成物：artifacts/occt/box-fillet.step、box-fillet.brep、demo.json，以及 shaded/face-id/edge-id PNG。
- 已查看结果 shaded 图，圆角外观正常；示例本身仍使用确定性 CPU 像素选择，没有运行真实 VLM。

导出边界回归：包含 NUL 的路径在 C++/OS 文件接口中会被截断，可能绕过临时文件路径并写到已有文件。
先以保护文件的测试复现（旧行为错误地返回成功），随后在所有导入/导出路径入口拒绝空字符串和 NUL；
STEP 和两种原生格式的回归测试均通过，已有文件保持原样。

本轮完成的是 **CPU 拾取 + OCCT 精确 STEP 的工作流**。GPU Picking、SMLib STEP bridge、
跨编辑 Naming、持久版本恢复、真实 Agent host 和外部内核对 STEP 的独立验证仍未完成。
