# 命令协议 v0.1

本地 UTF-8 JSONL。`id` 可选，C++ 服务会原样回显。
成功：`{"ok":true,"result":...}`；失败：`{"ok":false,"error":{"code":"command_failed","message":"..."}}`。
错误分类当前较粗，后续细分 conflict / invalid-input / kernel-failed / unsupported。
不把内核原始对象、指针或内存地址序列化。

创建文档返回 `document/revision/empty/units`；文档相关响应外层包含 `document/revision/data`，
mutation 和 state 的 data 为状态摘要。查询不会增加 revision。所有几何数值使用毫米，体积为 mm³。

| op | 输入（省略公共 op） | 输出 |
|---|---|---|
| capabilities | 无 | 内核、STEP/GPU/naming 标志 |
| document.create | 无 | 新 ID、revision=0 |
| document.state | document | empty、valid、faces、edges、volume |
| model.box | document、revision、size[3] | 新状态；替换当前单 body |
| model.fillet | document、revision、edge、radius | 新状态；单边圆角 |
| document.undo/redo | document、revision | 新状态与递增 revision |
| topology.edges | document | reference、length、采样折线 points |
| render.mesh | document | points、triangles（vertices + face reference） |
| render.snapshot | document、可选 directory | render_id、revision、camera、viewport、三张 PNG |
| render.pick | render_id、整数 x/y | 当前引用或 null（未命中） |
| export.native | document、path（.smb） | path、format=smb |
| import.step/export.step | document、path | unsupported；不写文件 |

snapshot/pick 是 Python 接入层扩展。渲染缓存只在此会话有效，revision 改变后 pick 失败。
图像原点左上，x 向右、y 向下，viewport 384×384；CPU edge 像素覆盖约 2px 半径，
深度阈值为 4 个像素对应的模型长度，不是生产 GPU picking 的精度保证。
Face/edge ID 图的 RGB 按小端 24-bit 编码；0 为背景，metadata 给出 ID→reference 映射。
普通 shaded 图不含实体标签。多条边的像素容差重叠时本原型只保留最后命中，后续需要候选消歧。

修改成功才提交快照、递增 revision 和截断 redo。参数、内核操作或验证失败保持历史不变。
图像与导出不修改模型。原生导出先写同目录临时文件，再 rename；异常清理临时文件。

reference 是不透明、revision 限定的 token，不承诺跨编辑存活。不得修改 token 内部内容，
不得把相同局部 slot 当作同一实体。undo/redo 后也需重新查询。

本协议是本地原型：尚无幂等键、会话复连、作业取消、资源配额、持久历史、访问控制和网络传输。
MCP 薄桥支持 initialize、notifications/initialized、ping、tools/list、tools/call；
没有声明 resources/prompts/HTTP 能力。tools/call 中 CAD 失败通过 isError 返回，不终止会话。
