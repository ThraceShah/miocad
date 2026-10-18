# cad-commander

C++17 应用核心门面。拥有文档与命令注册表，不依赖窗口、网络、标准输入输出、Python 或具体 Agent。
默认使用现有 SMLib adapter。`IKernel` 注入只作为内部可测试边界，不提供备用内核产品选项。

## 调用

链接 CMake target `cad-commander`，包含 `commander.h`：

```cpp
cad::Commander commander;
auto created = commander.execute({{"op", "document.create"}});
if (!created.at("ok").get<bool>()) { /* report created["error"] */ }
else {
    auto doc = created.at("result").at("document");
    auto result = commander.execute({{"op", "model.box"}, {"document", doc},
                                    {"revision", 0}, {"size", {10, 10, 10}}});
    // Handle result["ok"] and result["result"] / result["error"].
}
```

一个实例保留自己的文档集合；不同实例不共享文档。调用必须串行，多个实例也不能据此并发调用
同一个进程的 SMLib context。共享服务和并发调度属于 P3，尚未实现。

## 命令目录与错误

- `commands()` 返回本实例支持的核心命令描述；`execute({"op":"commands.list"})` 返回同一目录。
- 目录由 protocol/contract.json 提供；C++ 注册处理器并核对匹配，包含 ID、输入/输出 schema、effect 和前置条件。
- `effect`：query、session_write、document_write、external_write；原生导出不改变文档 revision。
- P2 将结构校验移到 cad-contract，C++ 和 Rust 共用中性契约，并校验成功输出后才发布文档。
- Rust DTO 以共享样例和真实 worker 测试校核，不维护第二份业务规则；没有代码生成器。
- `render.snapshot/pick` 是现有 Python 接入层扩展，不是本库的核心命令；P4 再迁移产品渲染路径。

保持既有 `ok/result/error`、文档响应与可选 request id 回显。
错误保留 `code=command_failed`，新增 `error.kind`，详见 [协议](../../docs/protocol.md)。
C++ worker 的 JSON 解析错误使用相同错误信封，由 worker 负责报告。

## 事务边界

文档修改先复制 Document 的历史元数据（底层 const Shape 快照共享），建模继续深拷贝候选 Shape。
操作、候选有效性检查、结果查询、响应构造全部成功后，才通过 noexcept swap 发布暂存 Document。
失败丢弃暂存状态，因此模型、revision、Undo/Redo 和 redo 分支保持不变。
Undo/Redo 也遵循该边界，避免恢复快照后查询失败却返回失败并改变文档。
创建文档先构造响应，再插入文档集合。外部文件写入采用同目录临时文件和 rename。

这是可恢复异常下的事务隔离，不提供内核段错误恢复、持久化或资源配额。
当前每次修改额外复制历史元数据，不复制整个历史中的几何；性能优化应基于后续测量。

## 验证

```sh
cmake --build build --target commander_test cad-server -j 6
ctest --test-dir build --output-on-failure
```

commander_contract 直接链接本库，使用真实 SMLib 验证建模与引用语义，
并使用故障注入验证候选修改后抛错、无效候选、响应生成失败、Undo/Redo 响应失败和部分导出失败。
