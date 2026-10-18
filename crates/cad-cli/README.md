# cad-cli

Rust JSONL 产品入口，使用 cad-access / cad-client 默认连接共享服务。
每行一个命令，每行一个结果；逐行模式的业务失败不终止流。--command 单条命令失败退出 1。
--help / --version 不连接服务，EOF 不关闭 daemon。运行与迁移见 ../../docs/migration-p4.md。
