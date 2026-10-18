# cad-ui-egui

与宿主无关的 egui 工作区、客户端读模型及预览/确认状态机。不依赖 eframe、Tokio、网络或进程 API。
宿主提供 Event，绘制返回 Action；相机、选择、悬停、草稿和布局均保存在本地。

- controller.rs：桌面/Web 共用的异步命令策略，Transport 提供连接、查询、发送和下载。
- model.rs：文档版本、草稿、候选 ticket、单次确认、取消与旧结果拒绝。
- widgets.rs：工具栏、文档树、参数/状态面板、纹理视口、局部像素拾取。
- lib.rs：宿主/后台之间的值类型事件和命令，不暴露内核对象。

原生 cad-gui 和 WASM cad-web 复用本 crate 的 UI、Controller 和读模型；传输/调度各自在宿主实现。
视口 CPU raster 与本机服务共用 cad-render；eframe/wgpu 的纹理显示不等于 GPU 几何渲染/拾取。
详情见 [桌面说明](../../docs/desktop-p5.md) 和 [Web 说明](../../docs/web-p6.md)。
