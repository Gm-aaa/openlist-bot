# -*- coding: UTF-8 -*-
from telegram import InlineKeyboardButton, InlineKeyboardMarkup, Update
from telegram.ext import (
    Application,
    CommandHandler,
    ContextTypes,
)

from tools.filters import check_is_admin


async def help_command(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """帮助命令"""
    if not await check_is_admin(update):
        return
    
    text = """
📚 OpenList Bot 命令帮助

🔍 搜索
/s 关键词 - 搜索网盘文件（支持按网盘类型筛选）
/sb 关键词 - 搜索种子/磁力链接
/sm 关键词 - 通过TMDB搜索电影/电视剧

🖥️ 存储浏览
/st - 浏览 OpenList 存储文件

📥 离线下载
/od - 开始离线下载
/ods - 查看下载状态
/cf - 配置默认下载工具和路径

🔄 文件刷新
/fl - 刷新OpenList/SmartStrm/Jellyfin

💡 使用说明：
• /s 搜索网盘文件，点击按钮可筛选网盘类型
• /sb 搜索种子，先选择索引器再搜索
• /sm 通过TMDB搜索电影，输入中文名，返回英文名和TMDB ID
• 点击🧲获取磁力链接，点击📥添加到离线下载
• /st 选择存储后可以浏览目录
• /od 选择工具和路径后输入下载链接
• /ods 查看当前下载进度
• /cf 配置默认的下载工具和路径
• /fl 刷新OpenList缓存/触发STRM生成/扫描媒体库
"""
    await update.message.reply_text(text)


async def start_command(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """开始命令"""
    text = """✅ OpenList Bot 已启动!

🔍 搜索
/s <关键词> - 搜索网盘文件
/sb <关键词> - 搜索种子
/sm <关键词> - TMDB电影搜索

📂 存储
/st - 浏览存储文件

📥 下载
/od - 离线下载
/ods - 下载状态
/cf - 配置下载设置

🔄 刷新
/fl - 刷新文件

📚 发送 /help 查看帮助"""
    await update.message.reply_text(text)


def register_handlers(app: Application):
    app.add_handler(CommandHandler("start", start_command))
    app.add_handler(CommandHandler("help", help_command))
