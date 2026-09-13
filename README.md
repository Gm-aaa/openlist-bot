# OpenList Bot

基于 [z-mio/Alist-bot](https://github.com/z-mio/Alist-bot) 二次开发的 Telegram 机器人，专为 OpenList 优化。采用 Rust 重写，具备极低内存占用和高性能异步处理能力。

## 项目亮点

本项目在原有的 Alist 管理功能基础上，针对 OpenList 进行了深度适配，并打通了从"搜索"到"下载"的全链路流程：

- **Rust 高性能**：内存占用从 Python 版本的 ~50MB 降至 **8MB 以下**，类型安全且无 GIL 限制。
- **深度适配 OpenList**：完美支持 OpenList API 存储管理与文件操作。
- **网盘搜索集成**：集成 PanSou，支持各大主流云盘搜索。
- **离线下载自动化**：支持通过 Telegram 直接提交磁力/链接到 OpenList 离线下载。
- **一键缓存刷新**：一键刷新 OpenList 文件缓存。
- **工程化优化**：采用 `tracing` 日志系统、`YAML` 配置管理，`tokio` 异步运行时与 `reqwest` 连接池。

## 功能命令

| 命令 | 说明 |
|:---:|:---|
| `/search <关键词>` | 搜索网盘文件（支持类型筛选） |
| `/browse` | 交互式浏览 OpenList 存储文件，支持删除、新建文件夹、上传文件 |
| `/download` | 离线下载与设置（新建任务 / 查看状态 / 配置） |
| `/refresh` | 刷新 OpenList 文件缓存 |
| `/help` | 查看详细指令帮助 |

## 部署方式

### 网页聊天工作台

项目内置中文网页聊天入口，支持手机和电脑，不加载外部字体、脚本或 CDN。
可以与 Telegram 同时运行，也可以只启动网页（`user.bot_token: ""` 或省略 `user`）。
所有网页用户使用配置的同一个管理员账号，拥有 OpenList 管理权限。

支持网盘搜索、存储浏览、文件直链、新建文件夹、删除、上传（每个文件最多 16 MB，拒绝同名覆盖）、离线下载、任务状态、缓存刷新和下载/搜索源设置。
普通文字作为搜索词，也可使用 `/search`、`/browse [路径]`、`/download`、`/tasks`、`/refresh [路径]`、`/settings`、`/help`。
这是针对 OpenList 的命令式聊天界面，不需要大模型 API。

1. 编译：`cargo build --release`。
2. 生成登录密码哈希：`./target/release/openlist-bot hash-password`，按提示输入至少 12 字符的密码。
3. 如需二级密码，再运行一次生成命令，使用另一组密码。
4. 在 `config.yaml` 中添加：

   ```yaml
   web:
     bind: "127.0.0.1:8080"
     username: "admin"
     password_hash: '$argon2id$...这里填写生成的完整哈希...'
     secondary_password_hash: null # 可选：填写第二个密码的完整哈希
     cookie_secure: true
   ```

   也支持明文登录密码：将 `password_hash` 替换为 `password: "your-password"`，不需要生成哈希。两项必须且只能配置一项。明文配置文件应仅允许管理员读取（例如 `chmod 600 config.yaml`）。

5. 启动程序，将 HTTPS 反向代理指向 `127.0.0.1:8080`，通过代理地址登录。
   仅本机/可信内网 HTTP 调试时，设置 `cookie_secure: false` 后访问 `http://127.0.0.1:8080`；HTTP 下启用 Secure Cookie 将无法保持登录。
   Docker 中设置 `web.bind: "0.0.0.0:8080"` 并添加端口映射，见 Compose 示例。

网页资源编译进二进制，不需要 Node.js 或单独的前端服务器。更改网页文件后需要重新编译。
本地 Docker 构建：`docker build -t openlist-bot:web .`；镜像内生成哈希：
`docker run --rm -it openlist-bot:web ./openlist-bot hash-password`。

安全与会话行为：

- 登录密码支持明文 `password` 或 Argon2id `password_hash`；二级密码使用 Argon2id 哈希。会话 Cookie 为 HttpOnly、SameSite=Strict，可启用 Secure。
- 配置二级密码后，删除、新建目录、上传、提交下载、刷新缓存和修改设置均逐次验证二级密码。
- 每个连接来源 IP 每分钟最多 10 次登录/二级密码验证；不信任转发 IP 头。反向代理后的用户共享该代理的限额，适合私人使用。
- 会话最长 8 小时；退出登录或重启服务后失效。账号或密码修改后需重启。
- 对话只保留在当前页面，刷新/关闭后清空。任务由 OpenList 继续执行；打开任务列表后每 30 秒更新，页面关闭后没有浏览器后台推送。
- 管理接口仅允许同源请求；文件名和搜索结果按文本渲染，不执行上游 HTML。OpenList/PanSou Token 不发送给浏览器。
- `config.yaml` 应只允许服务运行账号读写；网页设置会写回该文件，挂载时保留写权限。
- 本实例的网页上传使用串行锁，并强制刷新目录后检查重名；Telegram、其他客户端或其他实例写入 OpenList 不受该锁协调，不能视为跨客户端的原子禁止覆盖保证。

### 方案一：Docker 部署（推荐）

本项目支持 GitHub Packages (GHCR) 自动构建。部署前需要先获取并修改配置文件：

1. **下载示例配置文件**：
   ```bash
   wget https://raw.githubusercontent.com/Gm-aaa/openlist-bot/master/config.example.yaml -O config.yaml
   ```
2. **编辑 `config.yaml`**：
   根据文件内的注释填入你的 Token 和服务器地址。
3. **启动容器**：
   ```bash
   docker run -d \
     --name openlist-bot \
     -v $(pwd)/config.yaml:/app/config.yaml \
     -v $(pwd)/logs:/app/logs \
     --restart always \
     ghcr.io/gm-aaa/openlist-bot:latest
   ```

### 方案二：手动编译部署

1. **环境要求**：Rust 1.75+（推荐使用 [rustup](https://rustup.rs/)）
2. **克隆项目**：
   ```bash
   git clone https://github.com/Gm-aaa/openlist-bot.git
   cd openlist-bot
   ```
3. **编译项目**：
   ```bash
   cargo build --release
   ```
4. **配置文件**：
   ```bash
   cp config.example.yaml config.yaml
   # 编辑 config.yaml 填入你的 Token 和地址
   ```
5. **运行**：
   ```bash
   ./target/release/openlist-bot
   ```

## 配置指南

详细配置项说明请参考 [config.example.yaml](./config.example.yaml)。

### 关键配置获取路径：
- **Telegram Bot Token**: [@BotFather](https://t.me/BotFather)
- **OpenList Token**: 登录 OpenList -> 设置 -> 概览 -> API

## 开发计划 (Roadmap)

### 已实现功能
- [x] 适配 OpenList API 存储管理与交互式文件浏览
- [x] 网盘搜索 (PanSou) 集成
- [x] 离线下载：支持磁力/链接一键提交至 OpenList 离线下载
- [x] 配置管理：支持 YAML 配置文件与交互式修改配置项
- [x] Docker 支持：支持 GHCR 自动构建与多架构部署方案
- [x] **文件管理增强**：支持在 `/browse` 中直接删除文件/文件夹、新建文件夹、上传文件
- [x] **通知系统**：离线下载完成/失败后通过机器人自动推送通知

### 计划实现功能
- [ ] **字幕搜索集成**：支持中文字幕搜索功能（拟集成多个字幕库 API）
- [ ] **多用户权限管理**：更细粒度的管理员/成员使用权限控制

## 注意事项

- **安全性**：切勿将包含真实 Token 的 `config.yaml` 提交到公开仓库（项目已预设 `.gitignore`）。
- **代理**：如果你的服务器无法直接访问 Telegram API，请在 `config.yaml` 中配置 `proxy`。

## 致谢

感谢 [z-mio/Alist-bot](https://github.com/z-mio/Alist-bot) 提供的灵感和基础架构。

## 开源协议

MIT License
