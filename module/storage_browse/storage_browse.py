# -*- coding: UTF-8 -*-
import asyncio
import math
import urllib.parse
from telegram import InlineKeyboardButton, InlineKeyboardMarkup, Update
from telegram.ext import (
    Application,
    CommandHandler,
    CallbackQueryHandler,
    ContextTypes,
)

from api.openlist.openlist_api import openlist
from config.config import bot_cfg
from tools.filters import check_is_admin
from loguru import logger


STORAGE_CACHE: dict = {}
FILE_CACHE: dict = {}
PATH_COUNTER = 0
PATH_MAP = {}


async def send_typing(context: ContextTypes.DEFAULT_TYPE, chat_id: int):
    """发送 typing 状态"""
    try:
        await context.bot.send_chat_action(chat_id=chat_id, action="typing")
    except Exception:
        pass


def register_path(path: str) -> str:
    """注册路径并返回短ID"""
    global PATH_COUNTER
    PATH_COUNTER += 1
    path_id = str(PATH_COUNTER)
    PATH_MAP[path_id] = path
    return path_id


def get_path(path_id: str) -> str:
    """根据ID获取路径"""
    return PATH_MAP.get(path_id, "")


async def st_command(update: Update, context: ContextTypes.DEFAULT_TYPE, message=None):
    """存储浏览"""
    if not await check_is_admin(update):
        return
    
    # 确定使用哪个消息对象发送回复
    send_message = None
    if message:
        send_message = message.reply_text
    elif update.message:
        send_message = update.message.reply_text
    elif update.callback_query and update.callback_query.message:
        send_message = update.callback_query.message.reply_text
    
    if not send_message:
        logger.error("无法获取发送消息的方法")
        return
    
    try:
        await openlist.login()
        result = await openlist.storage_list()
        
        # 处理返回数据 - data 可能是 StorageInfo 对象列表
        storages = []
        if result.data:
            if isinstance(result.data, list):
                storages = result.data
            elif isinstance(result.data, dict):
                content = result.data.get("content", [])
                storages = content if isinstance(content, list) else []
        
        if not storages:
            await send_message("暂无存储")
            return
    except Exception as e:
        import traceback
        logger.error(f"获取存储列表失败: {e}")
        await send_message(f"获取存储列表失败: {e}")
        return
    
    # 构建存储列表按钮
    buttons = []
    for storage in storages:
        name = storage.remark or storage.mount_path or str(storage.id)
        # disabled is True means it's disabled
        status = "❎" if storage.disabled else "✅"
        # 回调数据包含 id 和 mount_path
        mount_path = storage.mount_path or "/"
        btn = InlineKeyboardButton(
            f"{status}{name}",
            callback_data=f"storage_{storage.id}:{mount_path}"
        )
        buttons.append([btn])
    
    # 添加取消按钮
    buttons.append([InlineKeyboardButton("❌ 取消", callback_data="st_cancel")])
    
    await send_message(
        "选择存储:",
        reply_markup=InlineKeyboardMarkup(buttons)
    )


async def storage_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理存储选择"""
    query = update.callback_query
    await query.answer()
    
    # 发送 typing 状态
    chat_id = query.message.chat.id
    asyncio.create_task(send_typing(context, chat_id))
    
    # 解析存储 ID 和 mount_path
    data = query.data.replace("storage_", "")
    parts = data.split(":", 1)
    storage_id = parts[0]
    mount_path = parts[1] if len(parts) > 1 else "/"
    
    logger.info(f"storage_callback: 选择存储 id={storage_id}, mount_path={mount_path}")
    
    # 获取该存储的根目录文件
    try:
        asyncio.create_task(send_typing(context, chat_id))
        result = await openlist.fs_list(mount_path)
        logger.info(f"fs_list 返回: code={result.code}, path={mount_path}")
        if result.data:
            content = result.data.get("content", []) if isinstance(result.data, dict) else []
            logger.info(f"内容数量: {len(content) if content else 0}")
        
        # 处理返回数据
        content = []
        if hasattr(result, 'data') and isinstance(result.data, dict):
            content = result.data.get("content", [])
        elif hasattr(result, 'data') and isinstance(result.data, list):
            content = result.data
    except Exception as e:
        await query.message.reply_text(f"获取文件列表失败: {e}")
        return
    
    if not content:
        await query.message.reply_text("该存储为空")
        return
    
    # 保存到缓存
    chat_id = query.message.chat.id
    FILE_CACHE[f"{chat_id}_storage"] = storage_id
    FILE_CACHE[f"{chat_id}_root_path"] = mount_path
    FILE_CACHE[f"{chat_id}_path"] = mount_path
    FILE_CACHE[f"{chat_id}_files"] = content
    
    # 构建文件列表
    text, buttons = await build_file_list(content, mount_path, query.message.message_id)
    
    await query.message.edit_text(
        text=text,
        reply_markup=InlineKeyboardMarkup(buttons)
    )


async def file_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理文件/目录点击"""
    query = update.callback_query
    await query.answer()
    
    data = query.data
    chat_id = query.message.chat.id
    msg_id = query.message.message_id
    
    # 发送 typing 状态
    asyncio.create_task(send_typing(context, chat_id))
    
    if data.startswith("file_"):
        # 点击文件，获取路径ID并查找实际路径
        path_id = data.replace("file_", "")
        file_path = get_path(path_id)
        if not file_path:
            await query.answer("路径已过期，请重新进入", show_alert=True)
            return
        asyncio.create_task(send_typing(context, chat_id))
        await copy_file_link(query, file_path)
        return
    
    if data.startswith("cd_"):
        # 目录导航 - 获取实际路径
        path_id = data.replace("cd_", "")
        path = get_path(path_id)
        if not path:
            await query.answer("路径已过期，请重新进入", show_alert=True)
            return
        logger.info(f"file_callback: cd_ 点击, path={path}")
        asyncio.create_task(send_typing(context, chat_id))
        await navigate_to_path(query, chat_id, msg_id, path, context)
        return
    
    if data.startswith("page_"):
        # 分页导航
        page = int(data.replace("page_", ""))
        current_path = FILE_CACHE.get(f"{chat_id}_path", "/")
        content = FILE_CACHE.get(f"{chat_id}_files", [])
        asyncio.create_task(send_typing(context, chat_id))
        text, buttons = await build_file_list(content, current_path, msg_id, page=page)
        await query.message.edit_text(text=text, reply_markup=InlineKeyboardMarkup(buttons))
        return
    
    if data == "back":
        logger.info(f"file_callback: back 点击")
        asyncio.create_task(send_typing(context, chat_id))
        # 返回上一层
        chat_id = query.message.chat.id
        current_path = FILE_CACHE.get(f"{chat_id}_path", "/")
        root_path = FILE_CACHE.get(f"{chat_id}_root_path", "/")
        
        logger.info(f"back: current_path={current_path}, root_path={root_path}")
        
        if current_path == root_path:
            # 已经是根目录，返回存储列表
            await st_command(update, context, message=query.message)
            return
        
        # 计算上级目录
        parent_path = "/".join(current_path.rstrip("/").split("/")[:-1])
        logger.info(f"back: 计算的 parent_path={parent_path}")
        
        if not parent_path.startswith(root_path.rstrip("/")):
            parent_path = root_path
        if not parent_path:
            parent_path = root_path
            
        logger.info(f"back: 最终 parent_path={parent_path}")
        await navigate_to_path(query, chat_id, msg_id, parent_path, context)


async def copy_file_link(query, file_path: str):
    """复制文件链接"""
    try:
        result = await openlist.fs_get(file_path)
        
        # 处理返回数据
        raw_url = ""
        if hasattr(result, 'data') and isinstance(result.data, dict):
            raw_url = result.data.get("raw_url", "")
        elif hasattr(result, 'data') and isinstance(result.data, list) and result.data:
            raw_url = getattr(result.data[0], 'raw_url', '')
        
        # 构建完整URL
        file_name = file_path.split("/")[-1]
        
        # 确保 URL 正确拼接
        web_url = bot_cfg.openlist_web.rstrip("/")
        if not file_path.startswith("/"):
            file_path = "/" + file_path
        full_url = f"{web_url}{file_path}"
        
        if raw_url:
            text = f"文件: `{file_name}`\n\n直链: {raw_url}\n\n打开链接: {full_url}"
        else:
            text = f"文件: `{file_name}`\n\n打开链接: {full_url}"
        
        await query.message.reply_text(text, disable_web_page_preview=True)
    except Exception as e:
        await query.message.reply_text(f"获取链接失败: {e}")


async def navigate_to_path(query, chat_id: int, msg_id: int, path: str, context: ContextTypes.DEFAULT_TYPE = None):
    """导航到指定路径"""
    logger.info(f"navigate_to_path: 进入 path={path}")
    
    # 发送 typing 状态
    if context:
        asyncio.create_task(send_typing(context, chat_id))
    
    try:
        result = await openlist.fs_list(path)
        logger.info(f"fs_list 返回: code={result.code}, message={result.message}")
        
        # 处理返回数据
        content = []
        if hasattr(result, 'data') and isinstance(result.data, dict):
            content = result.data.get("content", [])
        elif hasattr(result, 'data') and isinstance(result.data, list):
            content = result.data
    except Exception as e:
        await query.message.reply_text(f"获取文件列表失败: {e}")
        return
    
    if not content:
        # 空目录也显示返回按钮
        text = f"📁 `{path}`\n\n(空目录)"
        path_parts = [p for p in path.strip("/").split("/") if p]
        buttons = []
        if len(path_parts) > 0:
            buttons.append([
                InlineKeyboardButton("⬅️ 返回上一级", callback_data="back")
            ])
        await query.message.edit_text(text=text, reply_markup=InlineKeyboardMarkup(buttons))
        return
    
    # 更新缓存
    FILE_CACHE[f"{chat_id}_path"] = path
    FILE_CACHE[f"{chat_id}_files"] = content
    
    text, buttons = await build_file_list(content, path, msg_id)
    
    await query.message.edit_text(
        text=text,
        reply_markup=InlineKeyboardMarkup(buttons)
    )


async def build_file_list(content, current_path: str, msg_id: int, page: int = 1, per_page: int = 10):
    """构建文件列表消息（分页）"""
    buttons = []
    
    # 分类文件和文件夹
    dirs = []
    files = []
    
    for item in content:
        # item can be a dict or an object
        if isinstance(item, dict):
            is_dir = item.get("is_dir", False)
            name = item.get("name", "")
            size = item.get("size", 0)
        else:
            is_dir = getattr(item, "is_dir", False)
            name = getattr(item, "name", "")
            size = getattr(item, "size", 0)
        
        if is_dir:
            dirs.append({"name": name})
        else:
            files.append({"name": name, "size": size})
    
    # 计算总数和页数
    total_items = len(dirs) + len(files)
    total_pages = max(1, (total_items + per_page - 1) // per_page)
    page = max(1, min(page, total_pages))
    
    # 获取当前页的项目
    start_idx = (page - 1) * per_page
    end_idx = start_idx + per_page
    all_items = dirs + files
    page_items = all_items[start_idx:end_idx]
    
    # 构建文本
    text = f"📁 `{current_path}`\n"
    text += f"第 {page}/{total_pages} 页，共 {total_items} 项\n\n"
    
    # 添加目录
    item_idx = 0
    for item in page_items:
        name = item["name"]
        if "size" in item:
            # 文件
            size = format_size(item["size"])
            if current_path.endswith("/"):
                file_path = f"{current_path}{name}"
            else:
                file_path = f"{current_path}/{name}"
            path_id = register_path(file_path)
            buttons.append([
                InlineKeyboardButton(f"📄 {name} ({size})", callback_data=f"file_{path_id}")
            ])
        else:
            # 目录
            if current_path.endswith("/"):
                btn_path = f"{current_path}{name}/"
            else:
                btn_path = f"{current_path}/{name}/"
            path_id = register_path(btn_path)
            buttons.append([
                InlineKeyboardButton(f"📁 {name}", callback_data=f"cd_{path_id}")
            ])
        item_idx += 1
    
    # 添加分页按钮
    page_buttons = []
    if page > 1:
        page_buttons.append(InlineKeyboardButton("⬅️ 上一页", callback_data=f"page_{page-1}"))
    if page < total_pages:
        page_buttons.append(InlineKeyboardButton("➡️ 下一页", callback_data=f"page_{page+1}"))
    if page_buttons:
        buttons.append(page_buttons)
    
    # 添加返回按钮 - 始终显示（只要不是根目录）
    path_parts = [p for p in current_path.strip("/").split("/") if p]
    if len(path_parts) > 0:
        buttons.append([
            InlineKeyboardButton("⬅️ 返回上一级", callback_data="back")
        ])
    
    return text, buttons


def format_size(size):
    """格式化文件大小"""
    try:
        size = float(size)
    except (ValueError, TypeError):
        return "0B"
    
    for unit in ['B', 'KB', 'MB', 'GB', 'TB']:
        if size < 1024:
            return f"{size:.1f}{unit}"
        size /= 1024
    return f"{size:.1f}PB"


def register_handlers(app: Application):
    app.add_handler(CommandHandler("st", st_command))
    app.add_handler(CallbackQueryHandler(storage_callback, pattern=r"^storage_"))
    app.add_handler(CallbackQueryHandler(file_callback, pattern=r"^(file_|cd_|back|page_)"))
    app.add_handler(CallbackQueryHandler(st_cancel_callback, pattern=r"^st_cancel$"))


async def st_cancel_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """取消存储浏览"""
    query = update.callback_query
    await query.answer()
    await query.message.edit_text("❌ 已取消")
