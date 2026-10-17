# SMLib 调查（2026-10-15）

## 结论与证据边界

当前内核路线为 SMLib。编译未修改的官方 SMLib + SM_API 子集，
以建模、事务、拓扑选择和渲染验证推进 MVP。STEP 导出不再作为准入门槛，
OCCT 不作为备选内核。不能把社区封装的能力视为官方 API 的能力。
当前调查对应首次拉取的 main HEAD / v1.0.0：
`c9c5979f04cea8c38213004d4f04df0dcfb40d05`，根目录 VERSION 为 `1.0.0`。
Git submodule 固定该提交；“最新”指调查时的远端 HEAD，而不是持续追踪分支。

## 官方来源

- [仓库与构建说明](https://github.com/NVIDIA-Omniverse/usd-brep)
- [Apache-2.0](https://github.com/NVIDIA-Omniverse/usd-brep/blob/c9c5979f04cea8c38213004d4f04df0dcfb40d05/LICENSE)
- [第三方清单](https://github.com/NVIDIA-Omniverse/usd-brep/blob/c9c5979f04cea8c38213004d4f04df0dcfb40d05/THIRD_PARTY_NOTICES.md)
- 源码：`source/SM_API/inc/SmApi{General,Queries,Brep,ImportExport}.h`
- 生命周期：`source/SMLib/inc/Sm{Brep,Context,Attribute,Object}.h`
- [官方已知问题](https://github.com/NVIDIA-Omniverse/usd-brep/blob/c9c5979f04cea8c38213004d4f04df0dcfb40d05/KNOWN_ISSUES.md)

## 许可证与供应链

NVIDIA 主体代码采用 Apache-2.0；repo 启动脚本等有 MIT 例外。分发需要保留相关许可证、
版权与适用 NOTICE，修改需说明。完整 USD 构建的依赖还包括 OpenUSD、oneTBB、Python、
pybind11 等，各自许可证须分别处理。这里的独立内核构建不链接 USD、不使用 Python bindings、
不开启 TBB，不代表完整上游发行包也只有一个许可证。尚未为本项目自行选择主许可证。
第三方内核作为只读 submodule 引用，禁止将其源码混入自有修改。

社区 `awv-informatik/smlibpy` 提供 MIT 封装和 STEP，但其 CMake 会替换若干内核文件
（`third_party/patches`）。本次只阅读其构建方案，没有采用补丁，也没有把它作为官方基线。
其 STEP translator 不在当前实施范围。

## 构建与平台

上游使用 `repo.sh` / Packman / Premake：先 `./prebuild.sh` 再 `./repo.sh build`。
完整构建会自动获取 OpenUSD、Python 3.12、pybind11、Premake 和 repo 工具；USD schema
资源另由 prebuild 阶段生成。Linux 要求 build-essential；Windows 使用宿主 MSVC / SDK，
上游提示 VS2022 也需 v142。macOS 的文档支持 Apple Silicon，但默认不含完整 USD 路径，
并明确指出公开 Packman 缺少某些包，需本地依赖配置。

本项目提供 CMake 外部构建：只收集 `SMLib/src/*.cpp` 和 `SM_API/src/*.cpp`，
C++17，静态库，`NOMINMAX` / `SM_GRAPHICS_CALLBACKS=1`，关闭 TBB。
这些选项不修改任何第三方源码。这里只验证 Linux x86_64 / GCC 15.2 / CMake 4.2.3 /
Ninja / Release；Windows、macOS、ARM 和完整上游构建均未在本机验证。

## B-Rep 与生命周期

不能将结构简化为单纯的 Face 数组。`SmBrep` 管理 Region、Shell、Face、Loop、Edge、Vertex
以及 FaceUse / LoopUse / EdgeUse 等有向使用关系；几何曲线和曲面独立于拓扑，支持解析形状和
NURBS，并非每个实体都预先转换成 NURBS。支持非流形结构；本项目首轮只接纳闭合流形实体。

`SmContext` 必须比其对象活得更久。SM_API 有惰性创建的进程全局默认 context，初始化和遍历
计数并不保证任意并发安全。采用进程启动时初始化、单线程串行 worker；多并发建模使用多进程。
不能由单个 context 存活推断历史对象缓存自动被保留。

API 的所有权必须逐项检查：

- CreateBox / BrepCopy：返回新对象，由调用方释放。
- BooleanUnion / Difference / Intersection：成功时修改 A，删除 B，输出别名 A；失败时 A、B
  仍分配但可能已被修改，输出为空。不能复用失败操作的操作数。
- FilletEdges：原地修改 BRep，旧 Edge / Face / Curve 引用不能继续使用。
- Tessellate：返回新 PolyBrep；源 Face 标签是借用指针，只能在适配器内部、源模型存活时解析。

事务通过深拷贝候选对象实现，验证成功后才替换文档快照。Agent 只接触文档 ID、revision、
实体 token 和几何值，不能接触指针、内存地址或直接的数组索引引用。

## 能力验证方法

`tests/kernel_probe.cpp` 不依赖 GUI、Python CAD 封装或 GPU。

| 能力 | 核验方式 | 首轮边界 |
|---|---|---|
| Box | 10³，体积 1000、12 edges、manifold + AssertValid | 单个解析 Box |
| Boolean | 两个相交 Box，union 1500 / difference 500 / intersection 500 | 规则实体，不推广到任意退化输入 |
| Fillet | 单边 radius=1，体积 `1000 - 10*(1-pi/4)`，原模型不变 | 单个 Box edge |
| Tessellation | fillet 后非空 PolyBrep | 不证明任意模型的网格质量 |
| STEP import/export | 核查公共 API、实现目录和文件格式接口 | 官方 SM_API 未提供，未完成 STEP roundtrip |

执行结果记录于 `docs/validation.md`。没有把未运行的测试或未支持的文件格式标为通过。
`SmApiReadBrepFromFile` / `WriteBrepToFile` 处理 `.smb`；`SmSTEPSurface` 和
`EvaluateSTEP` 是几何参数约定，不能据名称宣称 STEP 文件支持。
`OCCT_BREP_IMPORT/EXPORT` 是 OpenCASCADE `.brep` 格式转换层，涉及 USD staging，
不是 STEP translator，也不是现成的 OCCT 建模 adapter。

## 拓扑演化与 Persistent Naming

`SmAttribute.h` 公开了通知/属性传播机制：split、merge、copy、construction/destruction、
geometry change、pre/post edit 等。属性分为可持久、可复制和临时类型。
文档同时说明传播依赖编辑函数正确发出 Notify；并非所有类型都有完整 split/merge 通知。
因此“有通知和持久属性”不等于“已有可直接使用的跨特征 Persistent Naming 服务”。

未在公共 SM_API 中发现统一 generated/modified/deleted 历史结果接口或可供 Agent 直接使用的
跨重建 naming resolver。首轮必须拒绝过期 revision 的引用；不能通过同序号重绑定，也不能把
几何签名相同当作身份相同。后续对 Box→fillet、split、merge、delete、copy、保存/重载建立
属性传播测试，构建一对多/多对一 lineage，并对歧义返回候选，不能静默猜测。
该回调方案尚未实测完整覆盖，属于高优先级风险。

## 历史技术对比（背景资料，不构成备选内核要求）

依据：[OCCT 仓库](https://github.com/Open-Cascade-SAS/OCCT)、
[OCAF 官方文档](https://github.com/Open-Cascade-SAS/OCCT/wiki/ocaf)、
[LGPL](https://github.com/Open-Cascade-SAS/OCCT/blob/master/LICENSE_LGPL_21.txt) 与
[exception](https://github.com/Open-Cascade-SAS/OCCT/blob/master/OCCT_LGPL_EXCEPTION.txt)。

| 维度 | SMLib | OpenCASCADE |
|---|---|---|
| 模型 | SmContext 下可变的 topology / geometry / uses | TopoDS shape、共享 TShape、orientation/location |
| 建模 | SM_API 包装 primitives / boolean / fillet / tessellation | BRepPrimAPI / BRepAlgoAPI / BRepFilletAPI / BRepMesh |
| 精确格式 | 原生 SMB；本次官方基线缺 STEP 文件层 | STEPControl / XDE，可独立于 GUI 使用 |
| 演化 | 属性传播通知，需验证与组织历史 | 算法 Generated / Modified / IsDeleted，配合 OCAF TNaming |
| 文档 | 本项目自建事务和历史 | 可使用 OCAF 文档/事务，但本项目仍保留独立 document 接口 |
| 构建 | 完整 repo/Packman 较重；纯内核可独立编译 | CMake，按模块选构建依赖 |
| 许可 | Apache-2.0 主体，依赖单独审查 | LGPL-2.1 + exception，分发义务不同 |
| 本次实测 | Linux 原版内核能力探针 | 未构建；这里是源码/官方资料比较，不是性能对比 |

OCAF naming 也需要正确登记操作历史与选择语义，不是任意参数变化下自动解决所有拓扑歧义。
暂不作性能优劣结论。2026-10-17 范围修订：仅推进 SMLib，不实施 OCCT adapter；
STEP 导出从需求中移除。上面的格式能力与历史对比仅作为调查事实保留。
