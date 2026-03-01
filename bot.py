# -*- coding: UTF-8 -*-
import asyncio
import logging
import os
import sys

import httpx
from loguru import logger
from telegram import Update, BotCommand
from telegram.ext import Application, CommandHandler, MessageHandler, CallbackQueryHandler, ContextTypes, filters

from api.openlist.openlist_api import openlist
from config.config import bot_cfg
from tools.filters import is_admin

# 设置环境变量代理（让 httpx 也使用代理）
if bot_cfg.proxy_enable and bot_cfg.hostname and bot_cfg.port:
    proxy_url = f"socks5://{bot_cfg.hostname}:{bot_cfg.port}"
    os.environ["HTTP_PROXY"] = proxy_url
    os.environ["HTTPS_PROXY"] = proxy_url
    os.environ["ALL_PROXY"] = proxy_url

log_level = bot_cfg.log_level or "INFO"

# 配置 Python logging 级别（控制第三方库的日志）
logging.basicConfig(level=log_level)
for lib in ["httpx", "telegram", "httpcore", "apscheduler", "telegram.ext"]:
    logging.getLogger(lib).setLevel(log_level)

logger.remove()
logger.add("logs/bot.log", rotation="5 MB", level=log_level)
logger.add(sys.stderr, level=log_level)
logger.info(f"日志等级: {log_level}")

# 构建 Application，添加代理支持
if bot_cfg.proxy_enable and bot_cfg.hostname and bot_cfg.port:
    from telegram.request import HTTPXRequest
    proxy_url = f"socks5://{bot_cfg.hostname}:{bot_cfg.port}"
    logger.info(f"使用代理: {proxy_url}")
    request = HTTPXRequest(
        proxy=proxy_url,
        read_timeout=120,
        connect_timeout=60,
        pool_timeout=30
    )
    application = (
        Application.builder()
        .token(bot_cfg.bot_token)
        .request(request)
        .build()
    )
else:
    application = Application.builder().token(bot_cfg.bot_token).build()


async def post_init(application: Application):
    admin_cmd = [
        BotCommand("s", "搜索网盘文件"),
        BotCommand("sb", "搜索种子"),
        BotCommand("sm", "TMDB电影搜索"),
        BotCommand("st", "浏览存储"),
        BotCommand("od", "离线下载"),
        BotCommand("ods", "下载状态"),
        BotCommand("cf", "配置下载设置"),
        BotCommand("fl", "刷新文件"),
        BotCommand("help", "查看帮助"),
    ]
    user_cmd = [
        BotCommand("s", "搜索网盘文件"),
        BotCommand("sb", "搜索种子"),
        BotCommand("sm", "TMDB电影搜索"),
        BotCommand("fl", "刷新文件"),
        BotCommand("help", "查看帮助"),
    ]
    await application.bot.set_my_commands(admin_cmd)
    await application.bot.set_my_commands(user_cmd)
    
    # 发送测试消息
    try:
        from config.config import bot_cfg
        await application.bot.send_message(
            chat_id=bot_cfg.admin,
            text="✅ Bot 已启动!\n\n"
                 "📚 可用命令:\n"
                 "/s <关键词> - 搜索网盘\n"
                 "/sb <关键词> - 搜索种子\n"
                 "/sm <关键词> - TMDB电影搜索\n"
                 "/st - 浏览存储\n"
                 "/od - 离线下载\n"
                 "/ods - 下载状态\n"
                 "/cf - 配置下载设置\n"
                 "/fl - 刷新文件\n"
                 "/help - 帮助"
        )
    except Exception as e:
        logger.error(f"发送测试消息失败: {e}")


async def menu_command(update: Update, context: ContextTypes.DEFAULT_TYPE):
    if not await is_admin(update.effective_user.id):
        return
    await update.message.reply_text("菜单设置成功，请退出聊天界面重新进入来刷新菜单")


async def toggle_proxy_command(update: Update, context: ContextTypes.DEFAULT_TYPE):
    if not await is_admin(update.effective_user.id):
        return
    bot_cfg.proxy_enable = not bot_cfg.proxy_enable
    status = "开启" if bot_cfg.proxy_enable else "关闭"
    await update.message.reply_text(f"代理已{status}，重启机器人后生效")


application.add_handler(CommandHandler("menu", menu_command))
application.add_handler(CommandHandler("px", toggle_proxy_command))


async def checking_async():
    """异步检查连接"""
    try:
        await openlist.login()
        logger.info("登录成功")
    except Exception as e:
        logger.error(f"连接失败: {e}")


def checking():
    """同步检查（仅启动时调用一次）"""
    import asyncio
    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)
    try:
        loop.run_until_complete(checking_async())
    except Exception as e:
        logger.error(f"检查失败: {e}")
    return logger.info("Bot开始运行...")


if __name__ == "__main__":
    logger.info("========== 开始启动 Bot ==========")
    checking()
    
    logger.info("开始初始化 handlers...")
    from module.init import init_task
    init_task(application)
    logger.info(f"已注册 handlers 数量: {len(application.handlers[0])}")
    
    logger.info("注册 post_init 回调...")
    application.post_init = post_init
    
    logger.info("准备调用 run_polling...")
    logger.info("========== Bot 准备就绪，等待消息 ==========")
    
    logger.info(f"代理: {bot_cfg.proxy_enable}, {bot_cfg.hostname}:{bot_cfg.port}")
    
    try:
        application.run_polling(
            drop_pending_updates=True,
            poll_interval=1.0,
            timeout=20
        )
    except Exception as e:
        logger.error(f"run_polling 异常: {e}")
        import traceback
        traceback.print_exc()
