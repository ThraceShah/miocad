# cad-service（P3/P4）

Rust 本机共享服务，管理 C++ `cad-server` worker。所有 SMLib 调用仍在 worker 内串行执行。
没有 Python 运行依赖；P4 Rust CLI/MCP 使用此服务并共享 Rust CPU 渲染。尚无 HTTP、WebSocket 或 GUI。

## 运行

```sh
cmake --build build -j 6
cargo build -p cad-service --locked
cargo run -p cad-client --example shared_session --locked -- target/debug/cad-service build/cad-server
```

SDK 示例自动发现或启动默认服务；另一个进程执行同一示例会附着同一服务并保留先前文档。
示例执行 Box/Fillet，并用第二个客户端拉取事件、刷新快照。示例退出后服务继续运行。
默认目录为 `$XDG_RUNTIME_DIR/miocad`，未设置时为 `$HOME/.cache/miocad`。
可传第三个参数选择专用 runtime 目录（测试不要使用默认目录）。显式关闭示例：

```sh
cargo run -p cad-client --example shared_session --locked -- target/debug/cad-service build/cad-server /tmp/my-cad-session --shutdown
```

此命令关闭指定目录中的服务；它不关闭其他目录的实例。也可以前台运行服务：

```sh
target/debug/cad-service --runtime-dir /tmp/my-cad-session --worker build/cad-server
```

前台 Ctrl-C 与 `SharedClient::shutdown()` 都会停止接收新请求、排空已接受请求，再停止 worker。
shutdown 响应表示 stopping，不能据此假定端口和文件已立即消失。内存文档随后丢失，重要模型先导出 SMB。
后台启动日志位于 runtime 目录下 `service.log`；前台日志写 stderr，stdout 不输出日志。

## 生命周期与身份

- 服务终生持有 `service.lock` 的 OS 文件锁；竞争进程不创建 worker，直接退出。
  不能删除/替换锁文件来“解锁”。崩溃由 OS 释放锁，遗留 endpoint 不视为活服务。
- 默认 worker 完成 hello 后才原子发布 `endpoint.json`，包含 loopback 地址、service ID 和随机 token。
  runtime 目录权限为 0700；这是同一用户的本机边界，不是远程多租户认证。
- SDK 校验服务 ID；worker 请求仍校验 worker/session ID。服务重启不会伪装为旧实例。
  SDK 不把旧请求迁到新实例，也不自动重放修改。
- 一个会话对应一个独立 worker；默认会话共享多个文档。`create_session(key)` 使用服务内幂等 key，
  重试得到同一会话；`attach(endpoint, client_id, Some(session_id))` 显式选择会话。
  独立批处理也可继续使用 `WorkerClient`，完全绕过共享服务。
- 文档通过 `snapshot()` 列举，再将 document ID 显式传给 Command；不维护全局“当前文档”。
  服务通过 Commander 的只读 `document.list` 获取权威文档清单及状态，不靠事件缓存推断完整文档列表，
  不在 Rust 复制权威模型。
- 一个活跃逻辑客户端使用独立 client ID；重连复用该 ID 和原 Request。不要让两个活跃连接共用计数器。
  `service-observer` 是服务保留的 worker 客户端 ID。

锁使用 Rust 1.90 标准库 `File::try_lock`，无额外锁库；行为依据
[标准库文件锁文档](https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock)。

## 事件与刷新

`events(after)` 是增量拉取，不是持续推送连接。每个会话有独立 cursor，成功提交后发布文档事件；
失败、查询、去重重放不重复发布。保留最近 64 条；调用方可按自己的刷新周期拉取。

1. 初次连接/重连：取 `snapshot()`，保存返回的 documents 和 cursor。
2. 拉取 `events(cursor)`；有变化时重新取 snapshot，并使用快照返回的 cursor。
3. 返回 `resync_required=true` 时丢弃旧缓存、取完整 snapshot，不依靠缺失的增量猜测状态。
4. snapshot 在同一 worker 锁内通过 document.list 查询全部文档和 cursor，避免状态/游标之间丢提交。
5. revision 冲突也刷新快照，然后由调用方重新决定操作；不得盲目换 revision 重发。

网络封装为 JSONL，每个 TCP 连接一次请求/响应，最大 16 MiB。
外层 `ServiceEnvelope` 绑定 service ID/token，内层保持 P2 Request/Response 和 worker 去重契约。
内层 `service_id=null`、hello 的 `shared_service=false` 描述 worker 通道；共享能力和身份由外层描述。
schema 仍来自 `protocol/contract.json` 的 ServiceEnvelope/ServiceReply/service_methods。

| 控制方法 | 用途 |
|---|---|
| ping | 就绪、服务 PID、默认会话 |
| sessions | 会话、worker ID/PID、ready/lost、原因、文档数和 cursor |
| create_session | 按幂等 key 创建隔离 worker |
| rpc | 向默认/指定会话发送原 worker 请求，包括 hello 和 request.status |
| snapshot | 会话内文档状态及一致 cursor |
| events | 从 cursor 拉取提交事件，检测缺失 |
| render_snapshot | 同一 worker 版本的场景截图，含三种 base64 PNG；可选文件输出 |
| render_pick | 共享帧缓存中的边/面拾取，先检查核心文档版本 |
| cancel | 明确返回 cancellation_unsupported |
| shutdown | 显式停止整个服务 |

## 故障与限制

客户端关闭 socket 或取消 future 不取消已接受的建模请求。SharedClient 对传输不确定性返回
OutcomeUnknown 并保留原 Request，逻辑客户端被标为不可继续；重新 attach 同一实例后先查询
request.status/快照，必要时显式重发原请求，使用既有去重。缓存过期时保守拒绝再次执行旧 ID。

worker EOF、畸形/错配响应或 60 秒 watchdog 超时后，会话进入 lost，worker 被终止；
正在执行的结果标为不确定，后续请求不伪报成功，也不自动建一个空 worker 顶替。
其他隔离会话继续工作。daemon 崩溃后可重新发现/启动，但得到新身份和空文档，不能恢复旧请求。
SDK 网络超时为 15 秒，可能先于内核结束；这同样要求重连核对，不能当作事务失败自动重做。

当前仅验证 Linux x86_64；随机 token 使用 `/dev/urandom`，Windows 移植需替换随机源并验证进程生命周期。
每服务最多 16 个会话、128 个同时处理的连接；每 worker 最多 128 个逻辑客户端（含服务观察者）。
worker 请求缓存仍为 64 条/16 MiB；会话及客户端记录本阶段不回收，以保留高水位拒绝旧请求的保证。
暂无磁盘恢复、浏览器传输、后台任务进度、单个内核调用安全抢占或大网格分块。


P4 产品入口、截图字段变化、MCP 配置及 release 包见 [迁移指南](../../docs/migration-p4.md)。
服务按会话缓存四帧，跨入口使用；渲染查询与帧发布在同一 worker 锁下，CPU rasterization 进入 blocking task。
边候选重叠返回 ambiguous/null，过期帧返回 stale_render，淘汰或跨会话返回 render_not_found。
