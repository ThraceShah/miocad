# cad-client（P2）

异步 Rust 客户端基础，依赖 cad-protocol，直接调用同一个 C++ Commander。
不包含几何算法、Python 启动器、MCP/CLI 产品入口或常驻共享服务。

- RpcClient：面向异步读写流的串行连接，可供后续 IPC/网络传输适配。
- WorkerClient：拥有独立子进程的开发/批处理连接，用于 P2 验收；不是未来默认共享模式。
- prepare/send：保留请求及目标实例，允许对已确认结果做显式同内容重发测试。
- execute：校验请求并执行一次；业务失败返回 Response.ok=false，传输不确定性返回 ClientError。
- status：查询本客户端此前请求的缓存结果。
- shutdown：关闭并等待自己拥有的 worker；Drop 配置 kill_on_drop 防止遗留独立子进程。
  不应将该所有权策略用于 P3 已附着的共享服务。

示例（先构建 C++ worker）：

```sh
cargo run -p cad-client --example box_fillet --locked -- build/cad-server
```

可通过 CAD_SERVER 指定集成测试的 worker；示例则接受显式可执行路径。
示例和 crate 的运行不需要 Python；仓库内部仍保留 Python 回归测试及待 P4 替换的旧入口。
