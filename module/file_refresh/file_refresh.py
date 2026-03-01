# -*- coding: UTF-8 -*-
import httpx
from telegram import Update, InlineKeyboardButton, InlineKeyboardMarkup
from telegram.ext import (
    Application,
    CommandHandler,
    CallbackQueryHandler,
    ContextTypes,
)

from api.openlist.openlist_api import openlist
from config.config import bot_cfg, od_cfg
from tools.filters import check_is_admin
from loguru import logger


async def fl_command(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """刷新命令 - 显示三个选项"""
    if not await check_is_admin(update):
        return
    
    if not update.message:
        return
    
    buttons = []
    
    # 一键刷新按钮
    if bot_cfg.openlist_host and bot_cfg.smartstrm_url and bot_cfg.jellyfin_host and bot_cfg.jellyfin_api_key:
        buttons.append([InlineKeyboardButton("🚀 一键刷新全部", callback_data="fl_refresh_all")])
    
    # OpenList 刷新按钮
    if bot_cfg.openlist_host:
        buttons.append([InlineKeyboardButton("🔄 刷新 OpenList 缓存", callback_data="fl_refresh_openlist")])
    
    # SmartStrm 触发按钮
    if bot_cfg.smartstrm_url:
        buttons.append([InlineKeyboardButton("📄 触发 SmartStrm", callback_data="fl_trigger_smartstrm")])
    
    # Jellyfin 扫描按钮
    if bot_cfg.jellyfin_host and bot_cfg.jellyfin_api_key:
        buttons.append([InlineKeyboardButton("📺 扫描 Jellyfin 媒体库", callback_data="fl_scan_jellyfin")])
    
    if not buttons:
        await update.message.reply_text("请先在配置文件中设置相关服务")
        return
    
    await update.message.reply_text(
        "请选择要执行的操作:",
        reply_markup=InlineKeyboardMarkup(buttons)
    )


async def refresh_all(query):
    """一键刷新全部 - 线性执行三个步骤"""
    results = []
    
    # 1. 刷新 OpenList
    try:
        await openlist.login()
        path = od_cfg.download_path if od_cfg and od_cfg.download_path else "/"
        result = await openlist.fs_get(path)
        if result.code == 200:
            results.append("✅ OpenList 缓存已刷新")
        else:
            results.append(f"❌ OpenList 刷新失败: {result.message}")
    except Exception as e:
        results.append(f"❌ OpenList 刷新失败: {e}")
    
    # 2. 触发 SmartStrm
    try:
        if not bot_cfg.smartstrm_url or not bot_cfg.task_name:
            results.append("⚠️ SmartStrm 未配置")
        else:
            path = od_cfg.download_path if od_cfg and od_cfg.download_path else "/"
            payload = {
                "event": "a_task",
                "delay": 0,
                "task": {
                    "name": bot_cfg.task_name,
                    "storage_path": path
                }
            }
            async with httpx.AsyncClient() as client:
                response = await client.post(bot_cfg.smartstrm_url, json=payload, timeout=30)
            if response.status_code in (200, 201, 202):
                results.append("✅ SmartStrm 任务已触发")
            else:
                results.append(f"❌ SmartStrm 触发失败: {response.status_code}")
    except Exception as e:
        results.append(f"❌ SmartStrm 触发失败: {e}")
    
    # 3. 扫描 Jellyfin
    try:
        if not bot_cfg.jellyfin_host or not bot_cfg.jellyfin_api_key:
            results.append("⚠️ Jellyfin 未配置")
        else:
            url = f"{bot_cfg.jellyfin_host}/Library/Refresh"
            headers = {
                "X-Emby-Token": bot_cfg.jellyfin_api_key
            }
            async with httpx.AsyncClient() as client:
                response = await client.post(url, headers=headers, timeout=30)
            if response.status_code in (200, 201, 204):
                results.append("✅ Jellyfin 媒体库扫描已触发")
            else:
                results.append(f"❌ Jellyfin 扫描失败: {response.status_code}")
    except Exception as e:
        results.append(f"❌ Jellyfin 扫描失败: {e}")
    
    # 返回结果
    await query.message.edit_text(
        "🔄 一键刷新完成\n\n" + "\n".join(results)
    )


async def fl_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理刷新回调"""
    query = update.callback_query
    
    try:
        await query.answer()
    except Exception:
        pass
    
    data = query.data
    
    if data == "fl_refresh_all":
        await refresh_all(query)
    elif data == "fl_refresh_openlist":
        await refresh_openlist(query)
    elif data == "fl_trigger_smartstrm":
        await trigger_smartstrm(query)
    elif data == "fl_scan_jellyfin":
        await scan_jellyfin(query)


async def refresh_openlist(query):
    """刷新 OpenList 缓存"""
    try:
        await openlist.login()
        
        # 使用配置中的下载路径刷新
        path = od_cfg.download_path if od_cfg and od_cfg.download_path else "/"
        
        # 调用 fs/get 来刷新特定路径的缓存
        result = await openlist.fs_get(path)
        
        if result.code == 200:
            await query.message.edit_text(f"✅ OpenList 缓存已刷新\n\n路径: {path}")
        else:
            await query.message.edit_text(f"❌ 刷新失败: {result.message}")
    except Exception as e:
        logger.error(f"刷新 OpenList 缓存失败: {e}")
        await query.message.edit_text(f"❌ 刷新失败: {e}")


async def trigger_smartstrm(query):
    """触发 SmartStrm 任务"""
    try:
        if not bot_cfg.smartstrm_url or not bot_cfg.task_name:
            await query.message.edit_text("❌ SmartStrm 未配置")
            return
        
        # 使用配置中的下载路径
        path = od_cfg.download_path if od_cfg and od_cfg.download_path else "/"
        
        payload = {
            "event": "a_task",
            "delay": 0,
            "task": {
                "name": bot_cfg.task_name,
                "storage_path": path
            }
        }
        
        async with httpx.AsyncClient() as client:
            response = await client.post(
                bot_cfg.smartstrm_url,
                json=payload,
                timeout=30
            )
        
        if response.status_code in (200, 201, 202):
            await query.message.edit_text(
                f"✅ SmartStrm 任务已触发\n\n"
                f"任务: {bot_cfg.task_name}\n"
                f"路径: {path}"
            )
        else:
            await query.message.edit_text(f"❌ 触发失败: {response.status_code}")
    except Exception as e:
        logger.error(f"触发 SmartStrm 失败: {e}")
        await query.message.edit_text(f"❌ 触发失败: {e}")


async def scan_jellyfin(query):
    """扫描 Jellyfin 媒体库"""
    try:
        if not bot_cfg.jellyfin_host or not bot_cfg.jellyfin_api_key:
            await query.message.edit_text("❌ Jellyfin 未配置")
            return
        
        url = f"{bot_cfg.jellyfin_host}/Library/Refresh"
        
        headers = {
            "X-Emby-Token": bot_cfg.jellyfin_api_key
        }
        
        async with httpx.AsyncClient() as client:
            response = await client.post(url, headers=headers, timeout=30)
        
        if response.status_code in (200, 201, 204):
            await query.message.edit_text("✅ Jellyfin 媒体库扫描已触发")
        else:
            await query.message.edit_text(f"❌ 扫描失败: {response.status_code}")
    except Exception as e:
        logger.error(f"扫描 Jellyfin 失败: {e}")
        await query.message.edit_text(f"❌ 扫描失败: {e}")


def register_handlers(app: Application):
    app.add_handler(CommandHandler("fl", fl_command))
    app.add_handler(CallbackQueryHandler(fl_callback, pattern=r"^fl_"))
