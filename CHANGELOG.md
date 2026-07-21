# 更新日志

本项目的重要变更都会记录在此文件中。

## [Unreleased] - 2026-07-21

### 修复（第二轮审阅）

- `fs_list` 此前对每一次目录浏览都强制传 `refresh: true`，导致每点一次目录/翻页/进子目录都让 OpenList 对该路径做一次全量重刷。对 115、阿里云等云盘既慢又易触发风控。现在浏览走 `refresh: false`，仅 `/refresh` 命令使用新增的 `fs_list_refresh`（`refresh: true`）。
- 群聊中「开始搜索资源」建立的 `AwaitingSearchQuery` 状态按 `chat_id` 存储且绕过管理员门禁，导致此后**任意用户**发的下一条消息都会被 bot 删除并当作搜索词消费。现在该状态记录发起者 `user_id`，仅消费发起者本人的消息，其他人的消息不删除、不处理。
- 离线下载/搜索下载/默认配置三处的下载工具按钮此前把工具名原样内嵌进 `callback_data`（`od_tool_{name}`/`sb_tool_{name}`/`cf_tool_{name}`），与此前修复的挂载路径同属一类：超长/含特殊字符的工具名会突破 Telegram 64 字节回调上限。现在统一经 `register_path` 生成短数字 id。
- 将 `s_dl_`/`s_cp_`/`search_filter_`/`cd_`/`file_`/`od_cd_`/`sb_cd_`/`cf_path_`/`cf_dir_`/`cf_select_`/`cfg_src_toggle_`/`ods_page_`/`ods_detail_` 等回调残留的 `data.replace(前缀, "")` 全部改为 `strip_prefix`，与既定规范一致，避免前缀在数据中间被误替换。
- 为 `is_member` 补充文档：空 `member` 列表 = 搜索完全公开（含 bot 所在群任意成员），如需限制访问必须显式填写白名单；`config.example.yaml` 同步加了醒目提示。

### 修复

- 存储列表按钮的回调数据此前直接内嵌原始挂载路径（`storage_{id}:{mount}`），中文等较长挂载路径会超出 Telegram 64 字节回调数据上限，导致 `/browse`（及"返回上一级"回到存储列表）整条消息发送失败、界面无任何响应。现在改用 `register_path` 生成短数字 id，消费端用 `strip_prefix` + `get_path` 还原路径。
- 新建文件夹、上传文件后重置操作状态（`op_state`）时，误把 `storage_id`/`root_path` 写成空串，致使此后"返回上一级"无法回到存储选择列表，而是错误地向文件系统根目录导航。现在重置时保留原有 `storage_id`/`root_path`。
- `callback_handler` 此前对内联按钮回调不做任何鉴权，群聊场景下任意成员都能驱动管理员发起的会话（浏览/删除文件、提交离线下载、修改配置）。现在浏览/下载/删除/改配置等回调仅管理员可用；成员仅可翻页、筛选、复制搜索结果及唤起搜索输入，越权点击返回"⛔ 无权限"。
- "确认下载"消息中的文件名此前用 MarkdownV2 代码块反引号包裹，但该消息并未按 MarkdownV2 发送，反引号被当作普通字符显示。现已去除。
- **删除文件功能完全失效**：删除确认回调 `del_confirm`/`del_cancel_msg` 与删除项回调 `del_{id}` 同以 `del_` 开头，而通用分支 `data.starts_with("del_")` 排在 `else if` 链最前，导致点击"确认删除"/"取消"都落入通用分支、`get_path` 查不到路径而静默无操作，`fs_remove` 永远不会被调用。现在通用分支排除这两个精确匹配，删除得以正常执行。
- "返回上一级"计算实际父目录时用字符串前缀 `parent.starts_with(root_path)` 判断是否仍在存储根内，会把 `/movies` 误判为 `/movie` 的下级。现在改用按路径段比较的 `path_is_within()`。
- 任务详情中的"❌ 取消任务"按钮既无对应回调处理、也无后端取消接口，是点击无效的死按钮，现已移除。
- 多个命令处理器（`/browse`、`/download`、任务状态、`/refresh`、`/search`）此前把 `config` 读锁一直持有到函数结束，跨越了登录/列目录/搜索等网络请求，会阻塞 `/px` 等需要写锁的配置变更。现在取出所需值后立即释放读锁。
- `fs_put_bytes` 上传时手动设置的 `Content-Length` 与 reqwest 依据 body 自动计算的值重复，已移除手动设置。

### 优化

- `strip_prefix` 替换脆弱的 `data.replace(前缀, "")`；`div_ceil`、`next_back` 等 clippy 风格清理，去除对 `Copy` 类型的多余 `clone`。

## [Unreleased] - 2026-07-20

### 修复

- 修复选择下载路径时无法显示正确内容的问题（涉及搜索下载 `sb_cd_`、离线下载 `od_cd_`、默认下载配置 `cf_path_`/`cf_dir_` 三处目录浏览）：
  - 文件条目此前以原始文件名作为按钮回调数据（`sb_file_*`/`od_file_*`），中文或较长文件名会超出 Telegram 64 字节回调数据上限，导致整条消息编辑请求被 `BUTTON_DATA_INVALID` 拒绝，界面停留在旧内容不更新；且该回调本身没有对应处理逻辑。路径选择器现在只展示目录，不再渲染文件按钮。
  - 目录列表此前先截取前 10 项再过滤目录，若目录下前几项均为文件，则文件夹按钮显示不全甚至完全不显示。现在改为先过滤出目录再截取前 10 个。
  - 当前路径插入 MarkdownV2 代码块时未做转义，路径含 `` ` `` 或 `\` 时消息解析失败、编辑不生效。现在统一使用 `escape_code` 转义。

### 优化

- 目录数超过 10 个时，在消息中提示“目录过多，仅显示前 10 个（共 N 个）”，避免误以为目录缺失。
