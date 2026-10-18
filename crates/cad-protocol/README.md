# cad-protocol（P2）

包含中性契约、结构校验、强类型标识、Finite、命令、结果、错误与事件 DTO。
共享来源为 [protocol/contract.json](../../protocol/contract.json)，语义和兼容范围见
[协议基线](../../protocol/README.md)。

依赖 serde / serde_json，无进程、网络、窗口或 SMLib 依赖。Rust DTO 通过目录覆盖、共享样例、
往返与真实 worker 契约测试校核；未引入代码生成器。未来浏览器适配可复用类型；本轮尚未验证 WASM 构建。
