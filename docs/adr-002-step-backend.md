# ADR-002：可选 OCCT adapter 先完成精确 STEP 闭环

日期：2026-10-16。状态：已实现并以 Linux OCCT 7.9.2 验证。

## 决策

保留 SMLib 默认后端，在同一 IKernel / Shape 边界增加可选 OCCT 7.9+ 后端。
`cad-server --kernel occt` 明确选择，未编译或未知后端启动失败，禁止静默 fallback。
CLI/MCP 通过同一个 worker 参数选择，不绑定 Agent。SMLib 不宣称具有 STEP 能力。

原因：官方 SMLib 的 OCCT BRep bridge 依赖 USD staging，不是直接 STEP 入口。
目前没有对这一转换链建立足够的验证。引入 OCCT adapter 可独立推进所需工作流，
也验证文档/协议不依赖单一内核，而不修改第三方代码或先引入整个 USD。
本次不是 SMLib→OCCT 转换：不同 worker 的文档分别属于所选后端，不能跨后端传递原生指针/对象。

## 接口与几何语义

- IKernel 提供 name、native_format、supports_step、import_step；Shape 提供 save_step。
- State 增加精确几何 bounds，单位 mm；既有字段与文档事务语义不变。
- OCCT clone 深拷贝几何，不共享可变 TShape；渲染先复制再附加 triangulation。
- OCCT fillet 在候选副本执行，随后做 BRepCheck_Analyzer（几何/精确检查）和正有限体积检查。
- STEP import 验证所有可转移根均成功，且仅含一个 solid、没有游离 face/edge/vertex。
  装配/多实体、开放面、混合几何明确拒绝；不截取第一个 solid，也不把失败根忽略。
- 输入单位按 STEP 元数据读入，reader target unit 固定为 mm；输出明确设置 mm。
- 输出使用 manifold_solid_brep / 精确曲面和曲线；不将三角网格包装成 STEP。
- 输出临时文件与目标同目录，成功 rename；失败清理临时文件，不改文档历史。
- OCCT 自身读入过程可能进行默认 shape processing；未建立任意复杂源文件逐实体公差保持的保证。

## 数值问题与解决

NURBS 圆柱是解析圆柱的精确 rational B-spline 表示。最初自适应 Gauss 体积积分
得到 62.8318441487，对解析期望 20*pi 的相对误差约 1.42e-7，未通过 1e-7 验收。
改为 VolumePropertiesGK，启用 B-spline knot span 分段，保持测试容差不变后通过。
没有修改 OCCT 内核，也没有通过放宽断言掩盖误差。

## 构建与依赖

默认 `CAD_ENABLE_OCCT=OFF`，原版 SMLib 路径仍可单独构建。
ON 时需 OCCT >= 7.9；直接寻找必要 toolkit，避免部分发行版 CMake 配置要求完整 VTK/GUI dev。
本机使用 Ubuntu apt 官方二进制包 7.9.2+dfsg-4，解包到 build/occt-sdk，未修改第三方文件。
Ubuntu 的完整 libocct-data-exchange-dev 依赖会拉入 VTK/Java 等；为本地 SDK 采用显式下载解包，
并在 packages.json 记录版本与 SHA256。脚本只适用于有对应包版本的 Debian/Ubuntu 环境。

应用只调用建模、mesh 和 STEP toolkit；发行版 TKDESTEP 仍会传递链接到 TKXCAF、TKV3d/TKService
以及 TBB 等 runtime。这是包装依赖，不要求显示服务器或 GPU。运行时库仍须可加载。
Linux 本地 SDK 用 build RPATH 解析传递依赖；构建产物不承诺脱离 SDK 目录独立分发。

OCCT 库的 LGPL-2.1 + exception 与其依赖许可需要单独保留；Apache-2.0 的 SMLib
许可不覆盖 OCCT。构建动态链接 OCCT，不复制或修改其源码。参见 THIRD_PARTY.md。

## 验证与剩余范围

STEP 测试先于实现落地，初始 4 个测试全部失败；实现后通过，并继续增加复杂样本。
Box、单边 Fillet、Cylinder、Sphere、Torus、NURBS cylinder 的 export/import 几何检查，
加单位换算、失败回滚、引用失效和 unsupported components 测试。
输入 fixture 由独立的 OCCT 直调程序生成，绕过 adapter；读写依然基于 OCCT，
尚未通过第二个 CAD 内核或外部商业软件做独立 roundtrip 验证。

STEP 保存的是交换几何，不是 AI-CAD 的文档历史。GPU Picking、持久 Naming、
磁盘版本恢复和实际 VLM 推理仍是后续工作，不因 STEP 完成而宣称实现。
