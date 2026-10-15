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
