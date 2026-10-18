# cad-contract / cad-wire（P2）

cad-contract 将中性 contract.json 嵌入 C++，提供 Commander 和协议端点共用的结构校验。
中性文件变化会触发 CMake 重新配置；测试核对嵌入版本与磁盘文件一致。

cad-wire 是独立 worker 的版本化 JSONL 端点：握手、实例/会话校验、请求身份预留、有限缓存、
高水位、结果查询和响应内事件。它调用 cad-commander，不另行实现建模规则。
它没有网络、窗口或订阅广播，也不是 P3 Rust 服务。

旧版 op JSONL 仍由 worker 直接送 Commander，新版 protocol 封装由端点校验后送同一 Commander。
详见 [协议基线](../../protocol/README.md)。
