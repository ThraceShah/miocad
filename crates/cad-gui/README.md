# cad-gui

原生 eframe/wgpu 宿主，连接与 CLI/MCP 相同的 cad-client SharedClient。
bridge.rs 将 CAD/事件轮询放到 Tokio 线程，将纯 CPU 视口计算放到独立线程；UI 线程不等待内核或网络。
队列限制为 32，视口任务取最新值；每个结果带文档版本/预览 ticket/视图 epoch。

关闭窗口只丢弃客户端连接；不发送 service.shutdown，也不销毁共享文档。
不确定修改不会重放，同实例重连查询 request.status 并刷新权威快照。
当前为 Linux X11、Vulkan/GLES；默认字体随 egui 编译，不依赖 Python 或外部字体服务。
运行、限制、截图与测试参见 [P5 桌面说明](../../docs/desktop-p5.md)。
