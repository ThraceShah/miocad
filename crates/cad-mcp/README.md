# cad-mcp

Rust MCP stdio 薄入口，使用 cad-access / cad-client。工具 cad_command 与原型同名。
MCP 2025-11-25 / 2025-06-18，initialize + initialized、ping、tools/list、tools/call。
结构化结果与 JSON text 一致，截图附 PNG ImageContent；CAD 失败为 isError，协议错误为 JSON-RPC error。
逐请求串行处理，不声明 tasks/HTTP/resources/prompts，不提供内核抢占取消。EOF 保留共享服务。
stdout 只有 JSON-RPC，诊断写 stderr。配置与限制见 ../../docs/migration-p4.md。
