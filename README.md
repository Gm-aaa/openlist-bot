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
