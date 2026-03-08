# -*- coding: UTF-8 -*-
import asyncio
import math
import time
import urllib.parse
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
from config.config import bot_cfg
from tools.filters import check_is_admin
from loguru import logger


STORAGE_CACHE: dict = {}
FILE_CACHE: dict = {}
PATH_COUNTER = 0
PATH_MAP = {}

STATE_NONE = 0
STATE_AWAITING_MKDIR = 1
STATE_AWAITING_UPLOAD = 2


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
        logger.error(f"获取存储列表失败: {e}")
        await send_message(f"获取存储列表失败: {e}")
        return

    buttons = []
    for storage in storages:
        name = storage.remark or storage.mount_path or str(storage.id)
        status = "❌" if storage.disabled else "✅"
        mount_path = storage.mount_path or "/"
        btn = InlineKeyboardButton(
            f"{status}{name}",
            callback_data=f"storage_{storage.id}:{mount_path}"
        )
        buttons.append([btn])

    buttons.append([InlineKeyboardButton("❌ 取消", callback_data="st_cancel")])

    await send_message(
        "选择存储:",
        reply_markup=InlineKeyboardMarkup(buttons)
    )


async def storage_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理存储选择"""
    query = update.callback_query
    await query.answer()

    chat_id = query.message.chat.id
    asyncio.create_task(send_typing(context, chat_id))

    data = query.data.replace("storage_", "")
    parts = data.split(":", 1)
    storage_id = parts[0]
    mount_path = parts[1] if len(parts) > 1 else "/"

    logger.info(f"storage_callback: 选择存储 id={storage_id}, mount_path={mount_path}")

    try:
        asyncio.create_task(send_typing(context, chat_id))
        result = await openlist.fs_list(mount_path)
        logger.info(f"fs_list 返回: code={result.code}, path={mount_path}")

        content = []
        if hasattr(result, 'data') and isinstance(result.data, dict):
            content = result.data.get("content", [])
        elif hasattr(result, 'data') and isinstance(result.data, list):
            content = result.data
    except Exception as e:
        await query.message.reply_text(f"获取文件列表失败: {e}")
        return

    FILE_CACHE[f"{chat_id}_storage"] = storage_id
    FILE_CACHE[f"{chat_id}_root_path"] = mount_path
    FILE_CACHE[f"{chat_id}_path"] = mount_path
    FILE_CACHE[f"{chat_id}_files"] = content

    msg_id = query.message.message_id
    text, buttons = await build_file_list(content, mount_path, msg_id, chat_id)

    await query.message.edit_text(
        text=text,
        reply_markup=InlineKeyboardMarkup(buttons),
        parse_mode="Markdown"
    )


async def file_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理文件/目录点击"""
    query = update.callback_query
    await query.answer()

    data = query.data
    chat_id = query.message.chat.id
    msg_id = query.message.message_id

    asyncio.create_task(send_typing(context, chat_id))

    if data.startswith("file_"):
        path_id = data.replace("file_", "")
        file_path = get_path(path_id)
        if not file_path:
            await query.answer("路径已过期，请重新进入", show_alert=True)
            return
        asyncio.create_task(send_typing(context, chat_id))
        await copy_file_link(query, file_path)
        return

    if data.startswith("cd_"):
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
        page = int(data.replace("page_", ""))
        current_path = FILE_CACHE.get(f"{chat_id}_path", "/")
        content = FILE_CACHE.get(f"{chat_id}_files", [])
        asyncio.create_task(send_typing(context, chat_id))
        text, buttons = await build_file_list(content, current_path, msg_id, chat_id, page=page)
        await query.message.edit_text(text=text, reply_markup=InlineKeyboardMarkup(buttons), parse_mode="Markdown")
        return

    if data == "back":
        logger.info(f"file_callback: back 点击")
        asyncio.create_task(send_typing(context, chat_id))
        current_path = FILE_CACHE.get(f"{chat_id}_path", "/")
        root_path = FILE_CACHE.get(f"{chat_id}_root_path", "/")

        logger.info(f"back: current_path={current_path}, root_path={root_path}")

        if current_path == root_path:
            await st_command(update, context, message=query.message)
            return

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

        raw_url = ""
        if hasattr(result, 'data') and isinstance(result.data, dict):
            raw_url = result.data.get("raw_url", "")
        elif hasattr(result, 'data') and isinstance(result.data, list) and result.data:
            raw_url = getattr(result.data[0], 'raw_url', '')

        file_name = file_path.split("/")[-1]

        web_url = bot_cfg.openlist_web.rstrip("/")
        if not file_path.startswith("/"):
            file_path = "/" + file_path
        full_url = f"{web_url}{file_path}"

        if raw_url:
            text = f"文件: `{file_name}`\n\n直链: {raw_url}\n\n打开链接: {full_url}"
        else:
            text = f"文件: `{file_name}`\n\n打开链接: {full_url}"

        await query.message.reply_text(text, parse_mode="Markdown", disable_web_page_preview=True)
    except Exception as e:
        await query.message.reply_text(f"获取链接失败: {e}")


async def navigate_to_path(query, chat_id: int, msg_id: int, path: str, context: ContextTypes.DEFAULT_TYPE = None):
    """导航到指定路径"""
    logger.info(f"navigate_to_path: 进入 path={path}")

    if context:
        asyncio.create_task(send_typing(context, chat_id))

    try:
        result = await openlist.fs_list(path)
        logger.info(f"fs_list 返回: code={result.code}, message={result.message}")

        content = []
        if hasattr(result, 'data') and isinstance(result.data, dict):
            content = result.data.get("content", [])
        elif hasattr(result, 'data') and isinstance(result.data, list):
            content = result.data
    except Exception as e:
        await query.message.reply_text(f"获取文件列表失败: {e}")
        return

    FILE_CACHE[f"{chat_id}_path"] = path
    FILE_CACHE[f"{chat_id}_files"] = content
    FILE_CACHE[f"{chat_id}_browse_msg_id"] = msg_id

    if not content:
        text = f"📁 `{path}`\n\n(空目录)"
        path_parts = [p for p in path.strip("/").split("/") if p]
        btns = []
        if path_parts:
            btns.append([InlineKeyboardButton("⬅️ 返回上一级", callback_data="back")])
        btns.append([
            InlineKeyboardButton("📁+ 新建文件夹", callback_data="st_mkdir"),
            InlineKeyboardButton("📤 上传文件", callback_data="st_upload"),
        ])
        await query.message.edit_text(text=text, reply_markup=InlineKeyboardMarkup(btns), parse_mode="Markdown")
        return

    text, buttons = await build_file_list(content, path, msg_id, chat_id)

    await query.message.edit_text(
        text=text,
        reply_markup=InlineKeyboardMarkup(buttons),
        parse_mode="Markdown"
    )


async def build_file_list(content, current_path: str, msg_id: int, chat_id: int = None, page: int = 1, per_page: int = 10):
    """构建文件列表消息（分页）"""
    if chat_id:
        FILE_CACHE[f"{chat_id}_browse_msg_id"] = msg_id

    dirs = []
    files = []

    for item in content:
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

    total_items = len(dirs) + len(files)
    total_pages = max(1, (total_items + per_page - 1) // per_page)
    page = max(1, min(page, total_pages))

    start_idx = (page - 1) * per_page
    end_idx = start_idx + per_page
    all_items = dirs + files
    page_items = all_items[start_idx:end_idx]

    text = f"📁 `{current_path}`\n"
    text += f"第 {page}/{total_pages} 页，共 {total_items} 项\n\n"

    buttons = []
    for item in page_items:
        name = item["name"]
        base = current_path.rstrip("/")
        if "size" in item:
            file_path = f"{base}/{name}"
            path_id = register_path(file_path)
            buttons.append([
                InlineKeyboardButton(f"📄 {name} ({format_size(item['size'])})", callback_data=f"file_{path_id}"),
                InlineKeyboardButton("🗑️", callback_data=f"del_{path_id}"),
            ])
        else:
            dir_path = f"{base}/{name}"
            path_id = register_path(dir_path)
            buttons.append([
                InlineKeyboardButton(f"📁 {name}", callback_data=f"cd_{path_id}"),
                InlineKeyboardButton("🗑️", callback_data=f"del_{path_id}"),
            ])

    page_buttons = []
    if page > 1:
        page_buttons.append(InlineKeyboardButton("⬅️ 上一页", callback_data=f"page_{page-1}"))
    if page < total_pages:
        page_buttons.append(InlineKeyboardButton("➡️ 下一页", callback_data=f"page_{page+1}"))
    if page_buttons:
        buttons.append(page_buttons)

    buttons.append([
        InlineKeyboardButton("📁+ 新建文件夹", callback_data="st_mkdir"),
        InlineKeyboardButton("📤 上传文件", callback_data="st_upload"),
    ])

    path_parts = [p for p in current_path.strip("/").split("/") if p]
    if path_parts:
        buttons.append([InlineKeyboardButton("⬅️ 返回上一级", callback_data="back")])

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


async def refresh_browse_message(context: ContextTypes.DEFAULT_TYPE, chat_id: int, msg_id: int, path: str):
    """刷新浏览消息"""
    try:
        result = await openlist.fs_list(path)
        content = []
        if hasattr(result, 'data') and isinstance(result.data, dict):
            content = result.data.get("content", [])
        elif hasattr(result, 'data') and isinstance(result.data, list):
            content = result.data

        FILE_CACHE[f"{chat_id}_files"] = content or []

        if not content:
            text = f"📁 `{path}`\n\n(空目录)"
            path_parts = [p for p in path.strip("/").split("/") if p]
            btns = []
            if path_parts:
                btns.append([InlineKeyboardButton("⬅️ 返回上一级", callback_data="back")])
            btns.append([
                InlineKeyboardButton("📁+ 新建文件夹", callback_data="st_mkdir"),
                InlineKeyboardButton("📤 上传文件", callback_data="st_upload"),
            ])
            await context.bot.edit_message_text(
                chat_id=chat_id, message_id=msg_id, text=text,
                reply_markup=InlineKeyboardMarkup(btns), parse_mode="Markdown",
            )
            return

        text, btns = await build_file_list(content, path, msg_id, chat_id)
        await context.bot.edit_message_text(
            chat_id=chat_id, message_id=msg_id, text=text,
            reply_markup=InlineKeyboardMarkup(btns), parse_mode="Markdown",
        )
    except Exception as e:
        logger.error(f"refresh_browse_message 失败: {e}")


# ─── Delete ───────────────────────────────────────────────────────────────────

async def del_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """显示删除确认"""
    query = update.callback_query
    await query.answer()
    chat_id = query.message.chat.id

    path_id = query.data[4:]  # strip "del_"
    full_path = get_path(path_id)
    if not full_path:
        await query.answer("路径已过期，请重新进入", show_alert=True)
        return

    full_path = full_path.rstrip("/")
    item_name = full_path.split("/")[-1]
    FILE_CACHE[f"{chat_id}_pending_delete_path"] = full_path

    btns = [[
        InlineKeyboardButton("✅ 确认删除", callback_data="del_confirm"),
        InlineKeyboardButton("❌ 取消", callback_data="del_cancel_msg"),
    ]]
    await query.message.reply_text(
        f"确认删除 `{item_name}`？\n\n此操作不可撤销！",
        reply_markup=InlineKeyboardMarkup(btns),
        parse_mode="Markdown",
    )


async def del_confirm_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """执行删除"""
    query = update.callback_query
    await query.answer()
    chat_id = query.message.chat.id

    full_path = FILE_CACHE.pop(f"{chat_id}_pending_delete_path", None)
    if not full_path:
        await query.message.edit_text("❌ 删除失败：找不到目标路径")
        return

    full_path = full_path.rstrip("/")
    dir_path = "/".join(full_path.split("/")[:-1]) or "/"
    item_name = full_path.split("/")[-1]

    try:
        result = await openlist.fs_remove(dir_path, [item_name])
        if result.code == 200:
            await query.message.edit_text(f"✅ 已删除 `{item_name}`", parse_mode="Markdown")
        else:
            await query.message.edit_text(f"❌ 删除失败: {result.message}")
            return
    except Exception as e:
        await query.message.edit_text(f"❌ 删除失败: {e}")
        return

    current_path = FILE_CACHE.get(f"{chat_id}_path", "/")
    browse_msg_id = FILE_CACHE.get(f"{chat_id}_browse_msg_id")
    if browse_msg_id:
        await refresh_browse_message(context, chat_id, browse_msg_id, current_path)


async def del_cancel_msg_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """取消删除"""
    query = update.callback_query
    await query.answer()
    chat_id = query.message.chat.id
    FILE_CACHE.pop(f"{chat_id}_pending_delete_path", None)
    await query.message.edit_text("❌ 已取消")


# ─── Mkdir ────────────────────────────────────────────────────────────────────

async def st_mkdir_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """触发新建文件夹"""
    query = update.callback_query
    await query.answer()
    chat_id = query.message.chat.id

    FILE_CACHE[f"{chat_id}_state"] = STATE_AWAITING_MKDIR
    btns = [[InlineKeyboardButton("❌ 取消", callback_data="st_op_cancel")]]
    prompt = await query.message.reply_text(
        "请输入新文件夹名称：",
        reply_markup=InlineKeyboardMarkup(btns),
    )
    FILE_CACHE[f"{chat_id}_prompt_msg_id"] = prompt.message_id


async def handle_text_input(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理文本输入（新建文件夹）"""
    chat_id = update.message.chat.id
    if FILE_CACHE.get(f"{chat_id}_state") != STATE_AWAITING_MKDIR:
        return

    folder_name = update.message.text.strip()
    if not folder_name:
        return

    FILE_CACHE[f"{chat_id}_state"] = STATE_NONE
    current_path = FILE_CACHE.get(f"{chat_id}_path", "/")
    new_path = f"{current_path.rstrip('/')}/{folder_name}"

    prompt_msg_id = FILE_CACHE.pop(f"{chat_id}_prompt_msg_id", None)
    if prompt_msg_id:
        try:
            await context.bot.delete_message(chat_id=chat_id, message_id=prompt_msg_id)
        except Exception:
            pass

    try:
        await update.message.delete()
    except Exception:
        pass

    try:
        result = await openlist.fs_mkdir(new_path)
        if result.code == 200:
            browse_msg_id = FILE_CACHE.get(f"{chat_id}_browse_msg_id")
            if browse_msg_id:
                await refresh_browse_message(context, chat_id, browse_msg_id, current_path)
        else:
            await context.bot.send_message(chat_id=chat_id, text=f"❌ 创建文件夹失败: {result.message}")
    except Exception as e:
        await context.bot.send_message(chat_id=chat_id, text=f"❌ 创建文件夹失败: {e}")


# ─── Upload ───────────────────────────────────────────────────────────────────

async def st_upload_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """触发文件上传"""
    query = update.callback_query
    await query.answer()
    chat_id = query.message.chat.id

    FILE_CACHE[f"{chat_id}_state"] = STATE_AWAITING_UPLOAD
    btns = [[InlineKeyboardButton("❌ 取消", callback_data="st_op_cancel")]]
    prompt = await query.message.reply_text(
        "请发送要上传的文件：",
        reply_markup=InlineKeyboardMarkup(btns),
    )
    FILE_CACHE[f"{chat_id}_prompt_msg_id"] = prompt.message_id


async def handle_document_input(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理文件上传"""
    chat_id = update.message.chat.id
    if FILE_CACHE.get(f"{chat_id}_state") != STATE_AWAITING_UPLOAD:
        return

    doc = update.message.document
    if not doc:
        return

    file_name = doc.file_name or f"upload_{int(time.time())}"
    current_path = FILE_CACHE.get(f"{chat_id}_path", "/")

    FILE_CACHE[f"{chat_id}_state"] = STATE_NONE

    prompt_msg_id = FILE_CACHE.pop(f"{chat_id}_prompt_msg_id", None)
    if prompt_msg_id:
        try:
            await context.bot.delete_message(chat_id=chat_id, message_id=prompt_msg_id)
        except Exception:
            pass

    prog_msg = await update.message.reply_text(f"⏳ 正在上传 `{file_name}`...", parse_mode="Markdown")

    try:
        tg_file = await doc.get_file()
        file_data = await tg_file.download_as_bytearray()
        result = await openlist.fs_put_bytes(bytes(file_data), current_path, file_name)
        if result.code == 200:
            await prog_msg.edit_text(f"✅ `{file_name}` 上传成功", parse_mode="Markdown")
        else:
            await prog_msg.edit_text(f"❌ 上传失败: {result.message}")
            return
    except Exception as e:
        await prog_msg.edit_text(f"❌ 上传失败: {e}")
        return

    browse_msg_id = FILE_CACHE.get(f"{chat_id}_browse_msg_id")
    if browse_msg_id:
        await refresh_browse_message(context, chat_id, browse_msg_id, current_path)


async def st_op_cancel_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """取消待处理的操作"""
    query = update.callback_query
    await query.answer()
    chat_id = query.message.chat.id
    FILE_CACHE[f"{chat_id}_state"] = STATE_NONE
    FILE_CACHE.pop(f"{chat_id}_prompt_msg_id", None)
    await query.message.edit_text("❌ 已取消")


# ─── Misc ─────────────────────────────────────────────────────────────────────

async def st_cancel_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """取消存储浏览"""
    query = update.callback_query
    await query.answer()
    await query.message.edit_text("❌ 已取消")


def register_handlers(app: Application):
    app.add_handler(CommandHandler("st", st_command))
    app.add_handler(CallbackQueryHandler(storage_callback, pattern=r"^storage_"))
    app.add_handler(CallbackQueryHandler(file_callback, pattern=r"^(file_|cd_|back|page_)"))
    app.add_handler(CallbackQueryHandler(st_cancel_callback, pattern=r"^st_cancel$"))
    app.add_handler(CallbackQueryHandler(del_callback, pattern=r"^del_\d+$"))
    app.add_handler(CallbackQueryHandler(del_confirm_callback, pattern=r"^del_confirm$"))
    app.add_handler(CallbackQueryHandler(del_cancel_msg_callback, pattern=r"^del_cancel_msg$"))
    app.add_handler(CallbackQueryHandler(st_mkdir_callback, pattern=r"^st_mkdir$"))
    app.add_handler(CallbackQueryHandler(st_upload_callback, pattern=r"^st_upload$"))
    app.add_handler(CallbackQueryHandler(st_op_cancel_callback, pattern=r"^st_op_cancel$"))
    app.add_handler(MessageHandler(filters.TEXT & ~filters.COMMAND, handle_text_input), group=2)
    app.add_handler(MessageHandler(filters.Document.ALL, handle_document_input), group=2)
