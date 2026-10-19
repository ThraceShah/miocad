# 平板与桌面工作区

2026-10-19。沿用 Rust egui / eframe 和客户端 wgpu，Web 优先 WebGPU、回退 WebGL2。

## 布局与操作

- 文档采用横向标签，`+` 创建文档。切换只改变本客户端活动文档，不删除文档或修改几何。
- 3D 视图上方为自绘矢量图标工具栏：Box、Fillet、参数/树显隐、缩放、Fit、Undo/Redo、刷新、重连及 SMB 导出。
- Box / Fillet 打开左侧 PropertyManager。竖屏参数栏约 242 屏幕点，可收起；浏览时不保留两侧栏，视图占满可用宽度。参数采用可点击输入框，避免滚动或触摸拖动数字时误改数值；非法尺寸禁用预览。
- FeatureManager 透明叠加在视图左侧，可折叠或隐藏，不占视图布局宽度。展示当前真实 body、face / edge 数、命名边和共享服务操作记录。操作记录来自带游标的服务事件，包含其他 Agent 提交；历史截断会明示。它是只读记录与拓扑树，不是可回编参数的特征依赖树。
- 单指拖动旋转；双指张开/捏合放大缩小，双指同向移动平移。手势中心是缩放锚点；双指剩一指时抑制旋转及点击，直到手指全部离开，避免误拾取。
- `Zoom + / Zoom −` 提供额外触屏入口；Fit 恢复居中及缩放，保留当前观察角度。缩放范围为 0.1–20。鼠标滚轮围绕指针缩放，Shift / 右键拖动平移，双击适应模型。
- Web 工作区使用动态视口高度与 `touch-action:none`，阻止画布手势变成整页缩放/滚动。渲染请求按比例缩至最长 768 像素，不再分别裁限宽高而拉伸竖屏模型；客户端 GPU 边线宽度仍按显示尺寸计算。

相机和选择属于客户端；预览、确认、失败回滚、revision 校验、Undo/Redo、共享会话及核心 MCP/CLI 协议保持不变。桌面也使用相同本地 depth-tested GPU 管线。

## 自动化和回归

浏览器的只读 `window.cadUI()` 返回实际 egui 控件边界、可用状态、视图区、相机和本地选择，供辅助工具/视觉自动化定位。它不是 CAD 命令入口，不包含凭证、服务端地址或内核指针。`tests/ui_driver.cjs` 依照实际控件边界操作；原有测试不再绑定旧侧栏的固定坐标。桌面只在显式设置 `CAD_GUI_PROBE_FILE` 时写出同类几何文件，默认不启用，不开额外端口。

```bash
PATH="$HOME/.cargo/bin:$PATH" scripts/build-web.sh
CAD_PLAYWRIGHT=/path/to/playwright CAD_BIN=target/release node tests/web_tablet.cjs
CAD_PLAYWRIGHT=/path/to/playwright CAD_BIN=target/release CAD_TEST_GPU=webgpu CAD_TEST_DPR=1 node tests/web_tablet.cjs
CAD_PLAYWRIGHT=/path/to/playwright ctest --test-dir build -R '^web_tablet_workspace$' --output-on-failure
```

`web_tablet_workspace` 使用独立临时 CAD 服务，创建两份测试文档，验证 768×1024 / 1280×820、DPR 1/2、双指放大与缩小、平移、方向保持、Fit、图标缩放、文档标签切换、左侧参数、真实输入值的预览/取消、模型状态不变和页面无异常。UI 单元测试另覆盖 820×1180，检查默认全宽、参数区左置、图标在视图上方及投影纵横比。

已有完整建模、断线核对、微小倒圆消歧、逐边拾取和桌面 Xvfb 测试迁移到新布局；生产测试只进行只读场景、客户端相机/选中操作。截图及 JSON 证据位于 Git 忽略的 `artifacts/tablet-*`。
