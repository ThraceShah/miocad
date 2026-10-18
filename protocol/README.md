# AI-CAD 协议基线（P2）

`contract.json` 是唯一的中性契约来源：命令目录、输入/输出、错误种类、版本、标识与事件结构。
CMake 将它原样嵌入 C++ `cad-contract`；Rust `cad-protocol` 使用 include_str! 嵌入同一文件。
Rust DTO 手工维护，通过目录覆盖、共享样例、往返和真实 worker 测试校核，不宣称已有代码生成器。
`commands.list` 中的 `$ref` 相对本文件的 `$defs` 解析；SDK 可通过 contract() 读取完整契约。

支持的结构校验子集：type、const、enum、required、properties、additionalProperties=false、items、
minItems/maxItems、minimum/maximum/exclusiveMinimum、minLength/maxLength、局部 $ref、anyOf/oneOf。
不接受外部引用；这不是通用 JSON Schema 实现。
数值必须有限；integer 要求 JSON 整数 token（1.0 不作为 revision/request_id）；
计数器限制为 0..9007199254740991，避免经过浏览器 JSON 层时丢失整数精度。
identifier 格式是非空 ASCII 字母、数字及 ._:-，最大 128 字节；长度按 UTF-8 字节计算。
浮点几何数值在 Rust 使用 Finite，serde_json 开启 float_roundtrip；序列化/解析必须保持有限 f64 的值，
不承诺保留 1000 与 1000.0 等数字的文本拼写。

## 握手与请求

P2 传输是本地子进程 UTF-8 JSONL，逐条异步请求/响应。先发送：

```json
{"protocol":"1.0","method":"hello","client_id":"agent-a","request_id":0}
```

成功响应含 protocol/client_id/worker_id/session_id/service_id、能力与去重限制。
当前一个 worker 只有一个内存 session；service_id=null 表示 P3 Rust 共享服务尚未实现。
客户端可选择自己的 client_id，但同一 worker 的同一 client_id 必须只有一个请求 ID 分配者。
hello 可重发，不能重置已接受请求的高水位；响应 last_request_id 返回该客户端高水位，
Rust 客户端从其后分配新请求 ID，不把重新握手当作 ID 归零。

随后执行：

```json
{"protocol":"1.0","method":"execute","request_id":1,"client_id":"agent-a","worker_id":"<hello.worker_id>","session_id":"<hello.session_id>","command":{"op":"document.create"}}
```

- RequestId 为每个 client_id 严格递增的正整数；0 仅用于 hello，可跳号，但旧号不能作为新请求。
- DocumentId 由核心生成。修改中 command.revision 就是 expected revision，保持既有命令字段名。
- WorkerId 标识本次核心实例，SessionId 标识其内存会话。重启后均改变；旧请求不得改换实例重放。
- ServiceId 已定义类型/响应字段，当前不分配真实服务实例，也不提供 attach/发现/自动启动。
- core 命令 ID 和 mm 单位不变；新版本与旧版无版本 JSONL 可分别调用同一个 Commander。
- protocol/method/request_id 是新封装的保留字段；现有 Python 继续发送旧 op 格式。

## 响应与事件

响应包含相同请求 ID、客户端与实例上下文，以及 ok、result 或 error、events、replayed。
错误 code 保留 command_failed，kind 来自中性错误枚举；retryable=false 表示不得自动重试。
成功模型创建/修改附带 document.created/document.changed 事件，携带 event_id、文档与 revision、
worker/session、来源 client/request 和命令。只读查询、失败及原生导出不产生文档事件。

P2 事件只随当前响应返回，不是订阅、持久日志或跨客户端广播。
重放缓存响应保留原 event_id；消费者按 (worker_id,event_id) 去重，不能当作第二次修改。
P3 再实现共享服务路由、订阅、漏事件恢复与重连同步。

## 结果查询、去重与断连

同一 worker 以 (client_id,request_id) 标识请求：

1. 通过版本/上下文/封装校验后，先保留请求身份，再调用 Commander；业务参数失败也保存终态响应。
2. 同 ID、相同解码后的请求内容，返回缓存响应并设置 replayed=true；不会再次调用几何操作。
3. 同 ID 改内容，返回 request_conflict。属性顺序不影响比较；不要改动数字表示后当成原请求重试。
4. 缓存 FIFO 最多 64 条/16 MiB，client 高水位不随缓存淘汰而删除。旧请求无缓存时返回
   outcome_unavailable，拒绝重新执行；此时必须核对文档，不能换个新 ID 盲目执行同一次修改。
5. 每 worker 最多记录 128 个 client_id；不包含身份认证或多用户隔离。缓存与高水位均不持久化。

查询已执行请求（同一 client_id，新的递增 request_id）：

```json
{"protocol":"1.0","method":"request.status","request_id":2,"client_id":"agent-a","worker_id":"<worker>","session_id":"<session>","target_request_id":1}
```

有缓存终态时返回 status=completed 和原始 response；completed 不代表建模成功，需检查其中 ok/error。
没有可靠记录时保守返回 outcome_unavailable，不宣称该操作从未执行。仅查询此前的 execute 请求。
如命令提交后响应组织异常，身份预留会阻止重做，返回 outcome_unknown；需要读文档核对结果。
这不是跨崩溃的 exactly-once，也没有持久化恢复保证。

Rust RpcClient 对 EOF、超时、截断/畸形响应、关联 ID 或实例不匹配返回 OutcomeUnknown，保留原 Request；
连接标为不可用，后续调用不能误读迟到响应。等待 future 被取消时也一样。
SDK 不自动重连、重发或启动新 worker 继续旧请求。已确认响应可显式 send 原 Request 检查去重。
P3 SharedClient 已提供实际重连：外层绑定同一 service ID，内层仍校验同一 worker/session 后查询/显式重发。

## 传输决定与大数据

worker 内部沿用 JSONL pipes；P3 使用 loopback 网络，P5 桌面作为普通客户端接入；Rust 异步读写不阻塞 UI 绘制。
读响应单帧限制 16 MiB，网格暂为 json.inline。超限不会伪装成成功，也不自动重试。
大型网格后续设计独立二进制/分块接口：需携带 document/revision、布局、大小和校验信息，
不把内核指针或进程内地址作为跨语言句柄。P2 未实现该接口或增量网格。

P3 本机共享服务使用 loopback TCP（仅本机绑定），外层 ServiceEnvelope/ServiceReply 及 service_methods
仍定义于 contract.json；建模 payload 不改 P2 协议。默认 worker 的 service_id 仍为 null，服务身份在外层绑定。
事件以游标增量拉取，快照用于重连/漏事件恢复。见 [服务契约与运行说明](../crates/cad-service/README.md)。
Unix socket/named pipe、浏览器 HTTP/WS 留待后续；当前未发布 Web 应用。

## 验证与范围

conformance.json 的 54 个样例由 C++、Rust 共用；错误、命令和输出类型均做一致性校核。
Rust 另测有限 f64 位级往返、真实 SMLib 工作流、断连/超时/取消/关联错误无自动重发。
C++ 继续保留 P1 故障注入和旧 CLI/MCP/预览回归。

P4 已提供 Rust MCP/CLI 和服务端 CPU 渲染；产品参数、schema 及字段迁移见 [迁移说明](../docs/migration-p4.md)。
服务 RenderSnapshot/RenderPick 与 product access_commands 仍来自同一 contract.json。

P5 已提供 egui 桌面与核心只读 model.preview / render.scene，契约仍来自同一 JSON。
预览不改正式 revision/history，候选引用不能用于修改正式模型；详见 [桌面说明](../docs/desktop-p5.md)。

不包括：WebUI、GPU 拾取、持久 Naming、磁盘恢复、STEP 或 OCCT。
