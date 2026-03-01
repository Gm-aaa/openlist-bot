# OpenList Bot

基于 [z-mio/Alist-bot](https://github.com/z-mio/Alist-bot) 二次开发的 Telegram 机器人，专为 OpenList 优化并集成多种媒体自动化工具。

## 项目亮点

本项目在原有的 Alist 管理功能基础上，针对 OpenList 进行了深度适配，并打通了从“搜索”到“下载”再到“媒体库刷新”的全链路自动化流程：

- **深度适配 OpenList**：完美支持 OpenList API 存储管理与文件操作。
- **多源搜索集成**：
  - 🚀 **网盘搜索**：集成 PanSou，支持各大主流云盘。
  - 🧲 **磁力搜索**：集成 Prowlarr，支持私有/公开索引器。
  - 🎬 **影视元数据**：集成 TMDB，搜索电影/电视剧并获取标准译名。
- **离线下载自动化**：支持通过 Telegram 直接提交磁力/链接到 OpenList 离线下载。
- **一键刷新链路**：一键触发 `OpenList 缓存刷新` -> `SmartStrm 生成 STRM` -> `Jellyfin 扫描`。
- **工程化优化**：采用 `loguru` 日志系统、`YAML` 配置管理，并优化了 `httpx` 连接池性能。

## 功能命令

| 命令 | 说明 |
|:---:|:---|
| `/s <关键词>` | 搜索网盘文件（支持类型筛选） |
| `/sb <关键词>` | 搜索种子/磁力链接 (Prowlarr) |
| `/sm <关键词>` | 通过 TMDB 搜索影视信息 |
| `/st` | 交互式浏览 OpenList 存储文件 |
| `/od` | 提交离线下载任务 |
| `/ods` | 查看离线下载进度状态 |
| `/cf` | 交互式配置默认下载设置 |
| `/fl` | 一键刷新文件链路 |
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

### 方案二：手动部署

1. **环境要求**：Python 3.11+
2. **克隆项目**：
   ```bash
   git clone https://github.com/Gm-aaa/openlist-bot.git
   cd openlist-bot
   ```
3. **安装依赖**：
   ```bash
   pip install -r requirements.txt
   ```
4. **配置文件**：
   ```bash
   cp config.example.yaml config.yaml
   # 编辑 config.yaml 填入你的 Token 和地址
   ```
5. **运行**：
   ```bash
   python bot.py
   ```

## 配置指南

详细配置项说明请参考 [config.example.yaml](./config.example.yaml)。

### 关键配置获取路径：
- **Telegram Bot Token**: [@BotFather](https://t.me/BotFather)
- **OpenList Token**: 登录 OpenList -> 设置 -> 概览 -> API
- **TMDB API Key**: [TheMovieDB Settings](https://www.themoviedb.org/settings/api)
- **Jellyfin API Key**: 控制台 -> 高级 -> API Keys

## 注意事项

- **安全性**：切勿将包含真实 Token 的 `config.yaml` 提交到公开仓库（项目已预设 `.gitignore`）。
- **代理**：如果你的服务器无法直接访问 Telegram API，请在 `config.yaml` 中配置 `proxy`。

## 致谢

感谢 [z-mio/Alist-bot](https://github.com/z-mio/Alist-bot) 提供的灵感和基础架构。

## 开源协议

MIT License
