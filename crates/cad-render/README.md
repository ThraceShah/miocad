# cad-render

共享 Rust 场景渲染库，输入 revision 一致的 Mesh / Edge；不调用内核、网络或窗口。
P7 在服务端使用 wgpu 两遍 compute 生成 Shaded / Face ID / Edge ID / 浮点深度，保留 CPU fallback。
默认 384×384，64..768；支持明确正交相机，Frame 的 PNG 和 ID 平面来自同一次渲染。

服务缓存最近四帧。桌面/Web/Agent 复用同一 Frame，可按 render ID 捕获；修改后或视口 epoch 过期的拾取被拒绝。
RGB/u32 ID 平面有界压缩传输；GPU 上传缓存按投影字节精确比较，场景缓存按文档 revision 更新。
边容差 2 px、深度使用表面斜率补偿及 0.05 px 偏差；歧义返回 null 与候选，不自动选第一个。
GPU 最多三个候选，溢出明确报告。它是三角化视图拾取，不是精确 B-Rep 射线求交。

适配器信息、fallback 原因、数值边界、测试和性能见 [P7 说明](../../docs/viewport-p7.md)。
