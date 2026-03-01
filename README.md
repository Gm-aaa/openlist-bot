# OpenList Bot

基于 [z-mio/Alist-bot](https://github.com/z-mio/Alist-bot) 二次开发的 Telegram 机器人，适配 OpenList 并增加多种实用功能。

## 项目背景

原项目 [z-mio/Alist-bot](https://github.com/z-mio/Alist-bot) 是针对 Alist 开发的 Telegram 管理 Bot。本项目在此基础上进行适配和扩展：

- 适配 OpenList API（原项目仅支持 Alist）
- 新增 TMDB 电影搜索功能
- 新增 Prowlarr 种子搜索功能
- 新增一键刷新文件流程（OpenList + SmartStrm + Jellyfin）
- 代码重构优化，提高稳定性和可维护性

## 功能概述

### 🔍 搜索
- `/s <关键词>` - 搜索网盘文件（支持按网盘类型筛选）
- `/sb <关键词>` - 搜索种子/磁力链接（Prowlarr）
- `/sm <关键词>` - 通过 TMDB 搜索电影/电视剧

### 📂 存储
- `/st` - 浏览 OpenList 存储文件

### 📥 下载
- `/od` - 离线下载
- `/ods` - 查看下载状态
- `/cf` - 配置默认下载工具和路径

### 🔄 文件刷新
- `/fl` - 一键刷新 OpenList 缓存 / 触发 SmartStrm / 扫描 Jellyfin

## 部署

### 环境要求

- Python 3.11+
- Telegram Bot Token
- OpenList 服务
- 网络代理（可选，用于访问外网）

### 1. 克隆项目

```bash
git clone https://github.com/your-repo/openlist-bot.git
cd openlist-bot
```

### 2. 创建虚拟环境（推荐）

```bash
python -m venv venv
source venv/bin/activate  # Linux/Mac
# 或
venv\Scripts\activate  # Windows
```

### 3. 安装依赖

```bash
pip install -r requirements.txt
```

### 4. 配置

复制并编辑配置文件：

```bash
cp config.example.yaml config.yaml
# 编辑 config.yaml
```

详细配置说明见下文。

### 5. 运行

```bash
python bot.py
```

### 6. 设置开机自启（Linux）

```bash
cat > /etc/systemd/system/openlist-bot.service <<EOF
[Unit]
Description=OpenList-bot Service
After=network.target

[Service]
User=root
WorkingDirectory=/path/to/openlist-bot
ExecStart=/path/to/openlist-bot/venv/bin/python bot.py
Restart=always

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable openlist-bot
systemctl start openlist-bot
```

## 配置文档

### 完整配置示例

```yaml
# 日志级别: DEBUG, INFO, WARNING, ERROR
log_level: INFO

# OpenList 配置
openlist:
  openlist_host: "https://your-openlist.domain.com"  # OpenList 地址
  openlist_token: "your-token"                        # OpenList Token
  openlist_web: "https://your-openlist.domain.com"   # OpenList Web 地址
  download_path: "/download"                           # 默认下载路径
  download_tool: "qbittorrent"                        # 默认下载工具

# 盘搜配置 (可选)
pansou:
  pansou_host: "https://pansou.domain.com"
  pansou_token: "your-pansou-token"  # 登录后获取的 JWT Token

# Prowlarr 配置 (可选，用于种子搜索)
prowlarr:
  prowlarr_host: "https://prowlarr.domain.com"
  prowlarr_api_key: "your-api-key"
  torrent_cache_max: 10

# TMDB 配置 (可选，用于电影搜索)
tmdb:
  tmdb_api_key: "your-tmdb-api-key"

# SmartStrm 配置 (可选，用于 STRM 生成)
smartstrm:
  smartstrm_url: "http://192.168.1.100:8024/webhook/your-webhook-id"
  task_name: "电影"

# Jellyfin 配置 (可选，用于媒体库扫描)
jellyfin:
  jellyfin_host: "http://192.168.1.100:8096"
  jellyfin_api_key: "your-jellyfin-api-key"

# 代理配置 (可选)
proxy:
  enable: true
  scheme: socks5  # http, socks5
  hostname: 127.0.0.1
  port: 10808

# 用户配置
user:
  admin: 123456789  # 管理员 Telegram ID
  bot_token: "your-bot-token"
  member: []       # 允许使用的用户ID，留空则所有人可用
```

### 配置说明

| 配置项 | 必填 | 说明 |
|--------|------|------|
| `log_level` | 否 | 日志级别，默认 INFO |
| `openlist.openlist_host` | ✅ | OpenList 服务地址 |
| `openlist.openlist_token` | ✅ | OpenList Token |
| `openlist.download_path` | 否 | 默认下载路径 |
| `openlist.download_tool` | 否 | 默认下载工具 |
| `pansou.*` | 否 | 盘搜功能配置 |
| `prowlarr.*` | 否 | 种子搜索功能配置 |
| `tmdb.tmdb_api_key` | 否 | TMDB API Key，从 themoviedb.org 获取 |
| `smartstrm.*` | 否 | SmartStrm 配置，用于 STRM 文件生成 |
| `jellyfin.*` | 否 | Jellyfin 配置，用于媒体库扫描 |
| `proxy.*` | 否 | SOCKS5/HTTP 代理配置 |
| `user.admin` | ✅ | 管理员 Telegram ID |
| `user.bot_token` | ✅ | Telegram Bot Token |

### 获取配置

1. **Telegram Bot Token**: @BotFather
2. **Telegram User ID**: @getuseridbot
3. **OpenList Token**: 登录 OpenList → 设置 → 概览 → API
4. **TMDB API Key**: https://www.themoviedb.org/settings/api
5. **Jellyfin API Key**: 控制台 → 高级 → API Keys → 添加 API Key

## 使用说明

### 搜索功能

```
/s 电影名称        # 搜索网盘文件
/sb 电影名称       # 搜索种子
/sm 电影名称       # 通过 TMDB 搜索电影
```

### 下载功能

```
/od              # 开始离线下载
/ods             # 查看下载状态
/cf              # 配置默认下载工具和路径
```

### 刷新功能

```
/fl              # 刷新文件（显示操作选项）
```

点击按钮可选择：
- 🚀 一键刷新全部（需要全部配置）
- 🔄 刷新 OpenList 缓存
- 📄 触发 SmartStrm
- 📺 扫描 Jellyfin

## 依赖

- python-telegram-bot~=20.8
- httpx~=0.26.0
- loguru~=0.7.2
- PyYAML~=6.0.1
- APScheduler~=3.10.1
- 其他见 requirements.txt

## 注意事项

1. 确保 OpenList 服务可访问
2. 代理配置用于访问外网服务（TMDB、Prowlarr 等）
3. 敏感配置请勿提交到公开仓库


