# -*- coding: UTF-8 -*-
from telegram import InlineKeyboardButton, InlineKeyboardMarkup, Update
from telegram.ext import (
    Application,
    CommandHandler,
    CallbackQueryHandler,
    ContextTypes,
    MessageHandler,
    filters,
)

from api.openlist.openlist_api import openlist
from config.config import bot_cfg, od_cfg
from tools.filters import check_is_admin
from loguru import logger

import yaml
import os

CF_CACHE = {}
STEP_SELECTING_TOOL = "tool"
STEP_BROWSING_PATH = "browse_path"
STEP_BROWSING_DIR = "browsing_dir"
STEP_ENTERING_URL = "url"
STEP_CONFIRMING = "confirm"
STEP_VIEW_CONFIG = "view_config"
STEP_EDIT_CONFIG = "edit_config"

CONFIG_FILE = "config.yaml"

# 复用 storage_browse 的路径注册函数
CF_PATH_MAP = {}
CF_PATH_COUNTER = 0


def cf_register_path(path: str) -> str:
    """注册路径并返回短ID"""
    global CF_PATH_COUNTER
    CF_PATH_COUNTER += 1
    path_id = str(CF_PATH_COUNTER)
    CF_PATH_MAP[path_id] = path
    return path_id


def cf_get_path(path_id: str) -> str:
    """根据ID获取路径"""
    return CF_PATH_MAP.get(path_id, "")


def get_current_config() -> tuple:
    """获取当前配置"""
    tool = od_cfg.download_tool if od_cfg and od_cfg.download_tool else "未设置"
    path = od_cfg.download_path if od_cfg and od_cfg.download_path else "/"
    return tool, path


async def cf_command(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """配置默认下载工具和路径"""
    if not await check_is_admin(update):
        return
    
    chat_id = update.effective_chat.id
    
    # 显示主菜单
    buttons = [
        [InlineKeyboardButton("🔧 修改下载设置", callback_data="cf_edit_download")],
        [InlineKeyboardButton("📋 查看全部配置", callback_data="cf_view_all")],
        [InlineKeyboardButton("✏️ 修改配置项", callback_data="cf_edit_item")],
    ]
    
    await update.message.reply_text(
        "⚙️ 配置管理\n\n请选择操作:",
        reply_markup=InlineKeyboardMarkup(buttons)
    )


async def cf_command_from_callback(query):
    """从回调返回主菜单"""
    chat_id = query.message.chat.id
    
    buttons = [
        [InlineKeyboardButton("🔧 修改下载设置", callback_data="cf_edit_download")],
        [InlineKeyboardButton("📋 查看全部配置", callback_data="cf_view_all")],
        [InlineKeyboardButton("✏️ 修改配置项", callback_data="cf_edit_item")],
    ]
    
    await query.message.edit_text(
        "⚙️ 配置管理\n\n请选择操作:",
        reply_markup=InlineKeyboardMarkup(buttons)
    )


async def cf_edit_download_config(query):
    """编辑下载设置（原 cf_command 的功能）"""
    chat_id = query.message.chat.id

    # 获取当前配置
    current_tool, current_path = get_current_config()
    
    try:
        await openlist.login()
        result = await openlist.get_offline_download_tools()
        
        if not result.data or len(result.data) == 0:
            await query.message.edit_text("暂无可用的离线下载工具")
            return
        
        # 显示当前配置
        current_text = f"""
📋 当前默认配置:

🔧 下载工具: {current_tool}
📂 下载路径: {current_path}

━━━━━━━━━━━━━━━━━━━
请选择新的下载工具:"""
        
        buttons = []
        for tool in result.data:
            if tool == current_tool:
                buttons.append([InlineKeyboardButton(f"✓ {tool}", callback_data=f"cf_tool_{tool}")])
            else:
                buttons.append([InlineKeyboardButton(tool, callback_data=f"cf_tool_{tool}")])
        
        await query.message.edit_text(
            current_text,
            reply_markup=InlineKeyboardMarkup(buttons)
        )
        
        CF_CACHE[chat_id] = {
            "step": STEP_SELECTING_TOOL,
            "message_id": query.message.message_id,
            "tool": current_tool,
            "path": current_path,
            "storage_id": None,
        }
        
    except Exception as e:
        logger.error(f"获取下载工具失败: {e}")
        await query.message.edit_text(f"获取下载工具失败: {e}")


async def cf_view_all_configs(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """查看全部配置"""
    query = update.callback_query
    
    try:
        await query.answer()
    except Exception:
        pass
    
    # 构建配置信息
    config_info = "📋 全部配置:\n\n"
    
    # OpenList
    config_info += "【OpenList】\n"
    config_info += f"• 地址: {bot_cfg.openlist_host or '未设置'}\n"
    config_info += f"• 默认下载路径: {od_cfg.download_path or '/'}\n"
    config_info += f"• 默认下载工具: {od_cfg.download_tool or '未设置'}\n\n"
    
    # Prowlarr
    config_info += "【Prowlarr 种子搜索】\n"
    config_info += f"• 地址: {bot_cfg.prowlarr_host or '未设置'}\n"
    config_info += f"• API Key: {'已设置' if bot_cfg.prowlarr_api_key else '未设置'}\n\n"
    
    # TMDB
    config_info += "【TMDB 电影搜索】\n"
    config_info += f"• API Key: {'已设置' if bot_cfg.tmdb_api_key else '未设置'}\n\n"
    
    # SmartStrm
    config_info += "【SmartStrm】\n"
    config_info += f"• Webhook URL: {'已设置' if bot_cfg.smartstrm_url else '未设置'}\n"
    config_info += f"• 任务名: {bot_cfg.task_name or '未设置'}\n\n"
    
    # Jellyfin
    config_info += "【Jellyfin】\n"
    config_info += f"• 地址: {bot_cfg.jellyfin_host or '未设置'}\n"
    config_info += f"• API Key: {'已设置' if bot_cfg.jellyfin_api_key else '未设置'}\n\n"
    
    # Proxy
    config_info += "【代理】\n"
    config_info += f"• 启用: {'是' if bot_cfg.proxy_enable else '否'}\n"
    if bot_cfg.proxy_enable:
        config_info += f"• 地址: {bot_cfg.hostname}:{bot_cfg.port}\n"
    
    buttons = [[InlineKeyboardButton("🔙 返回", callback_data="cf_back_menu")]]
    
    await query.message.edit_text(
        config_info,
        reply_markup=InlineKeyboardMarkup(buttons)
    )


# 可修改的配置项列表
CONFIG_ITEMS = {
    "openlist.download_path": "默认下载路径",
    "openlist.download_tool": "默认下载工具",
    "smartstrm.smartstrm_url": "SmartStrm Webhook URL",
    "smartstrm.task_name": "SmartStrm 任务名",
    "jellyfin.jellyfin_host": "Jellyfin 地址",
    "jellyfin.jellyfin_api_key": "Jellyfin API Key",
    "tmdb.tmdb_api_key": "TMDB API Key",
    "proxy.enable": "代理启用 (true/false)",
    "proxy.hostname": "代理地址",
    "proxy.port": "代理端口",
}


async def cf_edit_item_menu(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """显示可修改的配置项"""
    query = update.callback_query
    
    try:
        await query.answer()
    except Exception:
        pass
    
    buttons = []
    for key, name in CONFIG_ITEMS.items():
        buttons.append([InlineKeyboardButton(name, callback_data=f"cf_select_{key}")])
    
    buttons.append([InlineKeyboardButton("🔙 返回", callback_data="cf_back_menu")])
    
    await query.message.edit_text(
        "✏️ 选择要修改的配置项:",
        reply_markup=InlineKeyboardMarkup(buttons)
    )


async def cf_select_config_item(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """选择配置项后，等待用户输入新值"""
    query = update.callback_query
    
    try:
        await query.answer()
    except Exception:
        pass
    
    data = query.data
    config_key = data.replace("cf_select_", "")
    config_name = CONFIG_ITEMS.get(config_key, config_key)
    
    chat_id = query.message.chat.id
    
    # 保存当前正在修改的配置项
    CF_CACHE[chat_id] = {
        "step": STEP_EDIT_CONFIG,
        "config_key": config_key,
        "config_name": config_name,
    }
    
    # 获取当前值
    current_value = ""
    if config_key == "openlist.download_path":
        current_value = od_cfg.download_path or ""
    elif config_key == "openlist.download_tool":
        current_value = od_cfg.download_tool or ""
    elif config_key == "smartstrm.smartstrm_url":
        current_value = bot_cfg.smartstrm_url or ""
    elif config_key == "smartstrm.task_name":
        current_value = bot_cfg.task_name or ""
    elif config_key == "jellyfin.jellyfin_host":
        current_value = bot_cfg.jellyfin_host or ""
    elif config_key == "jellyfin.jellyfin_api_key":
        current_value = bot_cfg.jellyfin_api_key or ""
    elif config_key == "tmdb.tmdb_api_key":
        current_value = bot_cfg.tmdb_api_key or ""
    elif config_key == "proxy.enable":
        current_value = str(bot_cfg.proxy_enable) if bot_cfg.proxy_enable else "false"
    elif config_key == "proxy.hostname":
        current_value = bot_cfg.hostname or ""
    elif config_key == "proxy.port":
        current_value = str(bot_cfg.port) if bot_cfg.port else ""
    
    buttons = [[InlineKeyboardButton("❌ 取消", callback_data="cf_back_menu")]]
    
    await query.message.edit_text(
        f"✏️ 修改 {config_name}\n\n"
        f"当前值: `{current_value}`\n\n"
        f"请直接回复新的值（发送文本消息）",
        reply_markup=InlineKeyboardMarkup(buttons),
        parse_mode="Markdown"
    )
    
    # 获取当前配置
    current_tool, current_path = get_current_config()
    
    try:
        await openlist.login()
        result = await openlist.get_offline_download_tools()
        
        if not result.data or len(result.data) == 0:
            await update.message.reply_text("暂无可用的离线下载工具")
            return
        
        # 显示当前配置
        current_text = f"""
📋 当前默认配置:

🔧 下载工具: {current_tool}
📂 下载路径: {current_path}

━━━━━━━━━━━━━━━━━━━
请选择新的下载工具:"""
        
        buttons = []
        for tool in result.data:
            # 标记当前选中的工具
            if tool == current_tool:
                buttons.append([InlineKeyboardButton(f"✓ {tool}", callback_data=f"cf_tool_{tool}")])
            else:
                buttons.append([InlineKeyboardButton(tool, callback_data=f"cf_tool_{tool}")])
        
        msg = await update.message.reply_text(
            current_text,
            reply_markup=InlineKeyboardMarkup(buttons)
        )
        
        CF_CACHE[chat_id] = {
            "step": STEP_SELECTING_TOOL,
            "message_id": msg.message_id,
            "tool": current_tool,
            "path": current_path,
            "storage_id": None,
        }
        
    except Exception as e:
        logger.error(f"获取下载工具失败: {e}")
        await update.message.reply_text(f"获取下载工具失败: {e}")


async def cf_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理回调"""
    query = update.callback_query
    
    try:
        await query.answer()
    except Exception:
        pass
    
    chat_id = query.message.chat.id
    data = query.data
    
    # 主菜单按钮处理
    if data == "cf_edit_download":
        await cf_edit_download_config(query)
        return
    elif data == "cf_view_all":
        await cf_view_all_configs(update, context)
        return
    elif data == "cf_edit_item":
        await cf_edit_item_menu(update, context)
        return
    elif data == "cf_back_menu":
        await cf_command_from_callback(query)
        return
    elif data.startswith("cf_select_"):
        await cf_select_config_item(update, context)
        return
    
    if chat_id not in CF_CACHE:
        try:
            await query.message.edit_text("会话已过期，请重新输入 /cf")
        except Exception:
            pass
        return
    
    cache = CF_CACHE[chat_id]
    step = cache.get("step")
    
    # 选择工具
    if step == STEP_SELECTING_TOOL and data.startswith("cf_tool_"):
        tool = data.replace("cf_tool_", "")
        cache["tool"] = tool
        cache["step"] = STEP_BROWSING_PATH
        
        # 获取存储列表
        try:
            await openlist.login()
            result = await openlist.storage_list()
            storages = result.data if result.data else []
            
            buttons = []
            for storage in storages[:10]:
                storage_name = storage.mount_path or f"Storage {storage.id}"
                storage_path = storage.mount_path or "/"
                path_id = cf_register_path(storage_path)
                buttons.append([InlineKeyboardButton(f"📁 {storage_name}", callback_data=f"cf_path_{path_id}")])
            
            # 添加确认按钮
            buttons.append([InlineKeyboardButton("✅ 确认选择", callback_data="cf_confirm_path")])
            
            current_tool, current_path = get_current_config()
            
            await query.message.edit_text(
                f"✅ 已选择工具: {tool}\n\n"
                f"📋 当前默认配置:\n"
                f"🔧 工具: {current_tool}\n"
                f"📂 路径: {current_path}\n\n"
                f"━━━━━━━━━━━━━━━━━━━\n"
                f"请选择存储（点击文件夹进入）:",
                reply_markup=InlineKeyboardMarkup(buttons)
            )
        except Exception as e:
            logger.error(f"获取存储列表失败: {e}")
            cache["step"] = STEP_ENTERING_URL
            await query.message.edit_text(
                f"✅ 已选择工具: {tool}\n\n"
                f"请输入下载路径（默认 /）:",
            )
        return
    
    # 返回选择工具
    if data == "cf_back_tool":
        cache["step"] = STEP_SELECTING_TOOL
        cache["tool"] = od_cfg.download_tool if od_cfg and od_cfg.download_tool else None
        
        try:
            await openlist.login()
            result = await openlist.get_offline_download_tools()
            
            if not result.data or len(result.data) == 0:
                return
            
            current_tool, current_path = get_current_config()
            
            buttons = []
            for tool in result.data:
                if tool == cache.get("tool") or tool == current_tool:
                    buttons.append([InlineKeyboardButton(f"✓ {tool}", callback_data=f"cf_tool_{tool}")])
                else:
                    buttons.append([InlineKeyboardButton(tool, callback_data=f"cf_tool_{tool}")])
            
            await query.message.edit_text(
                f"📋 当前默认配置:\n"
                f"🔧 下载工具: {current_tool}\n"
                f"📂 下载路径: {current_path}\n\n"
                f"━━━━━━━━━━━━━━━━━━━\n"
                f"请选择新的下载工具:",
                reply_markup=InlineKeyboardMarkup(buttons)
            )
        except Exception as e:
            logger.error(f"返回工具选择失败: {e}")
        return
    
    # 选择存储 - 进入目录浏览 (使用路径ID)
    if step == STEP_BROWSING_PATH and data.startswith("cf_path_"):
        path_id = data.replace("cf_path_", "")
        path = cf_get_path(path_id)
        
        logger.info(f"选择存储: path_id={path_id}, path={path}")
        
        cache["path_id"] = path_id
        cache["path"] = path
        cache["step"] = STEP_BROWSING_DIR
        
        # 显示目录内容
        await show_directory(query, cache)
        return
    
    # 浏览目录 - 进入子目录 (使用路径ID)
    if step == STEP_BROWSING_DIR and data.startswith("cf_dir_"):
        dir_id = data.replace("cf_dir_", "")
        dir_path = cf_get_path(dir_id)
        
        cache["path_id"] = dir_id
        cache["path"] = dir_path
        
        # 显示目录内容
        await show_directory(query, cache)
        return
    
    # 返回上级目录
    if step == STEP_BROWSING_DIR and data == "cf_parent":
        current_path = cache.get("path", "/")
        
        # 处理根目录
        if current_path == "/" or current_path.count("/") <= 1:
            cache["path"] = f"/{cache.get('storage_id', '')}"
        else:
            # 获取上级目录
            parts = current_path.rstrip("/").rsplit("/", 1)
            cache["path"] = parts[0] if parts[0] else "/"
        
        # 显示目录内容
        await show_directory(query, cache)
        return
    
    # 返回选择存储
    if step == STEP_BROWSING_DIR and data == "cf_back_to_storages":
        cache["step"] = STEP_BROWSING_PATH
        
        try:
            await openlist.login()
            result = await openlist.storage_list()
            storages = result.data if result.data else []
            
            buttons = []
            for storage in storages[:10]:
                storage_name = storage.mount_path or f"Storage {storage.id}"
                buttons.append([InlineKeyboardButton(f"📁 {storage_name}", callback_data=f"cf_storage_{storage.id}")])
            
            buttons.append([InlineKeyboardButton("✅ 确认选择", callback_data="cf_confirm_path")])
            
            await query.message.edit_text(
                f"✅ 已选择工具: {cache['tool']}\n\n"
                f"📋 当前默认配置:\n"
                f"🔧 工具: {cache.get('tool')}\n"
                f"📂 路径: {cache.get('path')}\n\n"
                f"━━━━━━━━━━━━━━━━━━━\n"
                f"请选择存储（点击文件夹进入）:",
                reply_markup=InlineKeyboardMarkup(buttons)
            )
        except Exception as e:
            logger.error(f"返回存储列表失败: {e}")
        return
    
    # 确认路径
    if data == "cf_confirm_path":
        # 保存配置
        await save_config(cache["tool"], cache["path"])
        
        # 显示完成信息
        await query.message.edit_text(
            f"✅ 配置成功！\n\n"
            f"📋 当前默认配置:\n"
            f"🔧 下载工具: {cache['tool']}\n"
            f"📂 下载路径: {cache['path']}"
        )
        
        if chat_id in CF_CACHE:
            del CF_CACHE[chat_id]
        return
    
    # 确认配置（从工具选择界面直接确认）
    if data == "cf_confirm":
        # 保存配置
        await save_config(cache["tool"], cache.get("path", "/"))
        
        # 显示完成信息
        await query.message.edit_text(
            f"✅ 配置成功！\n\n"
            f"📋 当前默认配置:\n"
            f"🔧 下载工具: {cache['tool']}\n"
            f"📂 下载路径: {cache.get('path', '/')}"
        )
        
        if chat_id in CF_CACHE:
            del CF_CACHE[chat_id]
        return


async def show_directory(query, cache: dict):
    """显示目录内容"""
    try:
        path = cache.get("path", "/")
        
        # 获取文件列表
        result = await openlist.fs_list(path)
        
        files = result.data.get("content", []) if result.data else []
        
        # 只显示文件夹
        folders = [f for f in files if f.get("type") == 1]
        
        buttons = []
        
        # 显示子文件夹 - 使用路径ID
        for item in folders[:10]:
            name = item.get("name", "")
            full_path = f"{path.rstrip('/')}/{name}"
            path_id = cf_register_path(full_path)
            buttons.append([InlineKeyboardButton(f"📁 {name}", callback_data=f"cf_dir_{path_id}")])
        
        # 添加返回上级按钮 - 使用路径ID
        current_path_id = cache.get("path_id")
        if current_path_id:
            parent_path = path.rsplit("/", 1)[0] or "/"
            parent_path_id = cf_register_path(parent_path)
            buttons.append([InlineKeyboardButton("⬅️ 返回上级", callback_data=f"cf_dir_{parent_path_id}")])
        
        # 添加返回存储列表和确定按钮
        buttons.append([
            InlineKeyboardButton("🔙 选择存储", callback_data="cf_back_to_storages"),
            InlineKeyboardButton("✅ 确认路径", callback_data="cf_confirm_path")
        ])
        
        try:
            await query.message.edit_text(
                f"✅ 已选择工具: {cache['tool']}\n"
                f"📂 当前路径: `{path}`\n\n"
                f"点击文件夹进入，确认后点击完成",
                reply_markup=InlineKeyboardMarkup(buttons),
                parse_mode="Markdown"
            )
        except Exception as e:
            if "Message is not modified" not in str(e):
                raise
        
    except Exception as e:
        logger.error(f"显示目录失败: {e}")
        try:
            await query.message.edit_text(
                f"✅ 已选择工具: {cache['tool']}\n"
                f"📂 当前路径: `{cache.get('path')}`\n\n"
                f"无法获取目录内容，请确认或返回:",
                reply_markup=InlineKeyboardMarkup([
                    [InlineKeyboardButton("🔙 选择存储", callback_data="cf_back_to_storages")],
                    [InlineKeyboardButton("✅ 确认路径", callback_data="cf_confirm_path")]
                ]),
                parse_mode="Markdown"
            )
        except Exception:
            pass


async def cf_message(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理用户输入"""
    chat_id = update.effective_chat.id
    
    if chat_id not in CF_CACHE:
        return
    
    cache = CF_CACHE[chat_id]
    step = cache.get("step")
    
    # 处理编辑配置项
    if step == STEP_EDIT_CONFIG:
        config_key = cache.get("config_key")
        config_name = cache.get("config_name")
        new_value = update.message.text.strip()
        
        try:
            # 读取当前配置
            with open(CONFIG_FILE, "r", encoding="utf-8") as f:
                config_data = yaml.safe_load(f) or {}
            
            # 更新配置
            keys = config_key.split(".")
            temp = config_data
            for k in keys[:-1]:
                if k not in temp:
                    temp[k] = {}
                temp = temp[k]
            temp[keys[-1]] = new_value
            
            # 特殊处理布尔值
            if config_key == "proxy.enable":
                temp[keys[-1]] = new_value.lower() == "true"
            elif config_key == "proxy.port":
                try:
                    temp[keys[-1]] = int(new_value)
                except ValueError:
                    await update.message.reply_text("❌ 端口必须是数字")
                    return
            
            # 写入配置
            with open(CONFIG_FILE, "w", encoding="utf-8") as f:
                yaml.dump(config_data, f, allow_unicode=True, default_flow_style=False)
            
            # 刷新配置
            from config.config import reload_od_cfg
            reload_od_cfg()
            
            await update.message.reply_text(
                f"✅ 配置已更新！\n\n"
                f"• {config_name}: {new_value}\n\n"
                f"⚠️ 部分配置需要重启机器人后生效"
            )
            
            # 清理缓存
            if chat_id in CF_CACHE:
                del CF_CACHE[chat_id]
            
        except Exception as e:
            logger.error(f"保存配置失败: {e}")
            await update.message.reply_text(f"❌ 保存失败: {e}")
        return
    
    # 处理输入下载路径
    if step != STEP_ENTERING_URL:
        return
    
    path = update.message.text.strip()
    if not path.startswith("/"):
        path = "/" + path
    
    cache["path"] = path
    
    # 保存配置
    await save_config(cache["tool"], cache["path"])
    
    await update.message.reply_text(
        f"✅ 配置成功！\n\n"
        f"📋 当前默认配置:\n"
        f"🔧 下载工具: {cache['tool']}\n"
        f"📂 下载路径: {path}"
    )
    
    if chat_id in CF_CACHE:
        del CF_CACHE[chat_id]


async def save_config(tool: str, path: str):
    """保存配置到 config.yaml"""
    try:
        # 读取现有配置
        config_data = {}
        if os.path.exists(CONFIG_FILE):
            with open(CONFIG_FILE, "r", encoding="utf-8") as f:
                config_data = yaml.safe_load(f) or {}
        
        # 更新配置
        if "openlist" not in config_data:
            config_data["openlist"] = {}
        
        config_data["openlist"]["download_tool"] = tool
        config_data["openlist"]["download_path"] = path
        
        # 写入配置
        with open(CONFIG_FILE, "w", encoding="utf-8") as f:
            yaml.dump(config_data, f, allow_unicode=True, default_flow_style=False)
        
        # 刷新配置（让所有模块都使用新配置）
        from config.config import reload_od_cfg
        reload_od_cfg()
        
        logger.info(f"已保存配置: tool={tool}, path={path}")
        
    except Exception as e:
        logger.error(f"保存配置失败: {e}")
        raise


def register_handlers(app: Application):
    app.add_handler(CommandHandler("cf", cf_command))
    app.add_handler(CallbackQueryHandler(cf_callback, pattern=r"^cf_"))
    app.add_handler(MessageHandler(filters.TEXT & ~filters.COMMAND, cf_message))
