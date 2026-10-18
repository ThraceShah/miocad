# P6 WebUI

Rust egui/eframe 0.33.3 的 WASM 宿主复用 cad-ui-egui 工作区、Model、Controller、Action/Event 和 cad-render。
桌面和 Web 均依靠相同 Controller 实现预览/确认、乐观版本、事件游标、快照刷新及不确定结果核对。
原生 Bridge 只调度 Tokio、线程和 SharedClient；Web 宿主通过异步 HTTP 传输同一中性建模协议。
C++ SMLib 和权威 Document 留在服务器，无 Python 产品服务，无浏览器内核。

## 构建与本地启动

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
PATH="$HOME/.cargo/bin:$PATH" scripts/build-web.sh
HOST=127.0.0.1 PORT=18787 BASE_PATH=/apps/miocad/ \
  CAD_WEB_ORIGIN=http://127.0.0.1:18787 CAD_WEB_ASSETS="$PWD/artifacts/web" \
  ./target/release/cad-web --worker "$PWD/build/cad-server"
```

打开 http://127.0.0.1:18787/apps/miocad/ 。读取 CAD runtime-dir 的 web-access-code 文件，在页面输入解锁。
默认 runtime-dir 为 `$XDG_RUNTIME_DIR/miocad`，没有该变量时为 `$HOME/.cache/miocad`。
访问码是独立随机 192-bit 秘密，0600 保存，不是门户发布 token，也不是内部服务 token。
`--runtime-dir` 和 `--session` 可指定授权服务及现有会话；默认和同用户 CLI/MCP/桌面共享。
`--isolated` 不支持在 Web 网关隐式新建任意会话。需要隔离时先经本机 CLI 创建会话再部署专用网关。

所有外部 URL、资源、HTTP API、cookie Path 使用 BASE_PATH。直接访问网关时其路由同样带前缀。
门户当前会重写 cookie/Location，因此生产使用 strip-prefix=true、CAD_WEB_STRIP_PREFIX=1；网关内部路由和 cookie 使用根路径，由代理补上外部前缀。无斜杠入口重定向到规范入口；嵌套页面使用绝对 base。
生产 origin 默认 https://example.ts.net，可用 CAD_WEB_ORIGIN 明确配置。
网关只监听 loopback；HTTPS 由门户提供，不开放内部 JSONL worker。

## 授权和传输

- `POST api/login` 检查 Origin 和 X-CAD-Web 后核对访问码；最多 12 次/分钟，最多 16 个有效浏览器授权。
- 授权绑定 service/worker/session/client，cookie 为 HttpOnly、SameSite=Strict，HTTPS 下 Secure，8 小时过期。
  每个页面用随机 X-CAD-View 分配独立逻辑客户端，避免多标签页争用请求序号；每次授权最多 8 个页面身份，超出后重新解锁。
- 每个 API 均检查精确 Origin 和自定义请求头；不设置宽松 CORS。只接收 1 MiB 以内请求。
- 浏览器只允许中性建模/查询、request.status、指定会话快照/事件；不允许内部控制信封、切换会话、关闭服务、任意服务端文件路径。
- 重连在同一授权、同一 worker 上 hello 获取 high-water，查询原 request.status，并取权威快照；不自动重放建模。
  service/worker 身份变化须重新授权；授权过期或网关重启后重新输入访问码。
- `api/logout` 撤销当前授权。网关重启撤销全部内存授权；私有访问码保留。轮换访问码须更新私有文件并重启网关。
- 门户上的应用共用 origin，浏览器同源安全模型把其他同源应用视作可信；cookie Path 不构成同源应用之间的安全隔离。
  此阶段是同用户 Tailnet 私有协作服务，不是公网多租户账号系统。不要向不可信人员发放访问码。

API 使用 POST JSON，URL 均相对于 BASE_PATH；浏览器不接收内部 endpoint 地址或 token。
`hello/rpc` 返回中性 Response（外层附加网关已核验的 service_id），snapshot/events 使用现有 Service DTO。
`events` 采用 450 ms 轮询；游标间隙触发快照重同步。网络中断保留只读缓存、作废预览、禁用提交。
浏览器刷新会丢弃本地草稿并重新取权威快照，不自动提交或恢复旧请求。

## 视口与文件

视口调用相同 Rust CPU raster/picking，相机、选择、悬停和草稿只属于当前界面。
渲染在模块 Web Worker 中运行，主线程只应用最新 epoch/doc/revision 的帧；忙时合并待渲染视图。
eframe Web 用 WebGL2 显示场景纹理，桌面继续用 wgpu。GPU 几何/ID picking 仍属 P7。
当前 worker 帧采用有界 JSON 传递；复杂网格、移动设备性能和高 DPR 尚未验收。

Web 的 Native export 区域显示 Download SMB，不显示服务器路径输入框。
网关仅导出到随机私有临时目录，成功/失败后清理；下载上限 32 MiB，Blob + download 交给浏览器保存。
没有引入 STEP 或 OCCT；当前内核接口没有文档导入命令，因此不放置无实际能力的上传按钮。

## 发布与进程寿命

默认入口 https://example.ts.net/apps/miocad/，门户 https://example.ts.net/。
`scripts/publish-web.sh` 打包并部署；发布使用指定 tailnet-publish 技能。

CAD daemon 由独立 `miocad-cad-service.service` 管理，Web 网关由 `miocad-web.service` 管理，通过门户 proxy 收录。
Web 启用 CAD_WEB_ATTACH_ONLY=1，仅附着现有默认共享服务，不在 Web 的 systemd cgroup 内启动/拥有内核。
网关重启/再发布只撤销网页授权，不关闭桌面/Agent 的文档。CAD daemon 自身重启仍丢失内存文档；磁盘恢复尚未实现。
若已有原生客户端启动的默认 daemon，发布脚本复用它，不为部署网页而重启用户文档。

发布包包含 Rust 网关、WASM/JS、匹配服务/内核、协议版本和依赖许可证；没有 Python 运行时。
本机安装原生可执行文件，门户仅注册 loopback proxy，不上传二进制。首次 deploy_service 因门户对大型 base64 的正则校验栈溢出失败；检查日志后改用技能允许的持久本地服务 + proxy 路径，未修改门户代码。
发布脚本将匹配的 daemon/worker 安装到用户本地目录供独立 systemd 使用。

## 验证

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy -p cad-web --lib --target wasm32-unknown-unknown --locked -- -D warnings
ctest --test-dir build --output-on-failure
# 可用 -DCAD_ENABLE_WEB_TESTS=ON 将下述浏览器验收加入 CTest；默认关闭，避免无浏览器的构建环境失败。
# 内部测试工具，无产品依赖：
npm install --prefix artifacts/browser-tools playwright@1.56.1
artifacts/browser-tools/node_modules/.bin/playwright install chromium
CAD_PLAYWRIGHT="$PWD/artifacts/browser-tools/node_modules/playwright" node tests/web_smoke.cjs
```

Ubuntu 26.04 的 Playwright 1.56.1 安装可设置 PLAYWRIGHT_HOST_PLATFORM_OVERRIDE=ubuntu24.04-x64；这是测试浏览器的安装回退，不改变产品支持声明。
Rust gateway 集成测试使用真实 C++ worker、浏览器作用域和 Agent SDK；浏览器验收真实驱动 canvas，回执保存在 artifacts/p6-web。

显式线上验收（创建演示文档并重启 Web 网关，不重启 CAD daemon）：

```sh
CAD_PLAYWRIGHT="$PWD/artifacts/browser-tools/node_modules/playwright" node tests/web_deployment.cjs
```

如果测试 Chromium 继承的系统代理不支持 Tailnet，可用 CAD_TEST_PROXY 指定该本机代理；测试只在自己的 browser context 将目标 Tailnet 域名设为直连，不修改系统配置。
当前 worker 逻辑客户端总量有 128 上限，页面身份累计耗尽时拒绝新客户端；不会为释放额度自动重启共享服务。

P7 后续更新：Web 现改用服务端 wgpu/CPU Frame，Worker 处理异步 HTTP 和有界压缩平面；不再在浏览器另算 CPU ID 图。
新增独立渲染 relay，升级不重启原文档拥有者。本文 P6 的历史实现说明保留，当前架构以 [P7 说明](viewport-p7.md) 为准。
