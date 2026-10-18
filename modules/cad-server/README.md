# cad-server

P1 后的 C++ JSONL worker。main 只创建 Commander、读取/解析请求行、写出响应，
以及处理解析和传输错误。文档、命令注册/校验/分发、结果和业务错误由
[cad-commander](../cad-commander/README.md) 统一处理。

可执行文件仍为 `build/cad-server`，现有 Python MCP/CLI 调用方式不变。
当前是单进程串行、内存会话；不是 P3 的 Rust 共享服务。后续再演进为内部 core worker。
