# cad-render

纯 Rust CPU 场景渲染库，输入 revision 一致的 Mesh / Edge 折线；不调用内核、进程、网络或窗口。
cad-service 在同一 worker 锁内获取几何，使用 spawn_blocking 渲染并缓存最近四帧。
CLI/MCP 只通过 cad-client/cad-access 请求服务能力，不各自重实现渲染。

固定正交等轴测相机从 (+,+,+) 观察，按视口自动缩放，z-buffer 光照三角形 rasterization。
默认 384×384，可配置 64..768；生成 RGB8 PNG 的 shaded、face-id、edge-id 和帧内 ID 映射。
24-bit ID 按 RGB 小端编码，0 为背景/不可直接选取的歧义。边折线约 2px 覆盖和 4px 深度容差，
是 CPU 预览拾取，不是精确射线或 GPU picking。重叠候选标记 ambiguous，reference=null。

服务给帧附加 render/document/revision/worker/session 和相机、尺寸。每次 pick 都向核心验证 revision，
然后查询同一帧的映射；帧缓存跨入口共享，不作为长期拓扑命名。编辑后的引用还必须通过核心验证。
省略 directory 时没有磁盘写入；显式 directory 保存图片路径到 files，images 始终为 base64 PNG。

测试核对 PNG 像素 ID 与 pick 引用、可见性、背景、歧义、尺寸和坏网格；产品测试覆盖跨进程/会话与 stale/evicted 帧。
