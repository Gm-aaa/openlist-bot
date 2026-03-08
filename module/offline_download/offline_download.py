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


DOWNLOAD_CACHE = {}
STEP_SELECTING_TOOL = "tool"

# ─── 路径 ID 注册（避免 callback_data 超过 64 字节限制）─────────────────────────
OD_PATH_COUNTER = 0
OD_PATH_MAP: dict[str, str] = {}


def od_register_path(path: str) -> str:
    global OD_PATH_COUNTER
    OD_PATH_COUNTER += 1
    pid = str(OD_PATH_COUNTER)
    OD_PATH_MAP[pid] = path
    return pid


def od_get_path(pid: str) -> str:
    return OD_PATH_MAP.get(pid, "")


# ─── 下载完成通知 ──────────────────────────────────────────────────────────────
PREV_DONE_IDS: set[str] = set()
_COMPLETION_INITIALIZED = False


async def check_download_completion(context) -> None:
    """定时检查离线下载任务，完成时主动推送通知（JobQueue 回调）"""
    global PREV_DONE_IDS, _COMPLETION_INITIALIZED
    try:
        done = await openlist.get_offline_download_done_task()

        current_done_ids: set[str] = set()
        done_map: dict[str, dict] = {}
        if done.data and isinstance(done.data, list):
            for t in done.data:
                if t.get("id"):
                    tid = str(t["id"])
                    current_done_ids.add(tid)
                    done_map[tid] = t

        # 首次运行：记录已有完成任务，不触发通知
        if not _COMPLETION_INITIALIZED:
            PREV_DONE_IDS = current_done_ids
            _COMPLETION_INITIALIZED = True
            return

        newly_done = current_done_ids - PREV_DONE_IDS
        for task_id in newly_done:
            task = done_map[task_id]
            name, path = extract_file_info(task.get("name", ""))
            size = format_size(task.get("total_bytes", 0))
            err = task.get("error", "")
            if err:
                text = f"❌ 下载失败\n\n文件: {name}\n路径: {path}\n错误: {err}"
            else:
                text = f"✅ 下载完成\n\n文件: {name}\n大小: {size}\n路径: {path}"
            try:
                await context.bot.send_message(chat_id=bot_cfg.admin, text=text)
            except Exception as e:
                logger.error(f"发送下载通知失败: {e}")

        PREV_DONE_IDS = current_done_ids
    except Exception as e:
        logger.error(f"check_download_completion 失败: {e}")
STEP_BROWSING_PATH = "browse_path"
STEP_ENTERING_URL = "url"
STEP_CONFIRMING = "confirm"


async def start_download_with_url(message, tool: str, path: str, url: str):
    """从外部模块启动下载流程（预填URL，直接确认）"""
    chat_id = message.chat.id
    
    DOWNLOAD_CACHE[chat_id] = {
        "step": STEP_CONFIRMING,
        "message_id": message.message_id,
        "tool": tool,
        "path": path,
        "root_path": "/",
        "urls": [url]
    }
    
    await message.edit_text(
        f"📥 确认下载\n\n"
        f"工具: {tool}\n"
        f"路径: {path}\n"
        f"链接: {url}\n",
        reply_markup=InlineKeyboardMarkup([
            [InlineKeyboardButton("✅ 确认下载", callback_data="od_confirm")],
            [InlineKeyboardButton("❌ 取消", callback_data="od_cancel")]
        ])
    )


async def od_command(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """离线下载命令"""
    if not await check_is_admin(update):
        return
    
    chat_id = update.effective_chat.id
    
    try:
        await openlist.login()
        result = await openlist.get_offline_download_tools()
        
        if not result.data or len(result.data) == 0:
            await update.message.reply_text("暂无可用的离线下载工具")
            return
        
        buttons = []
        for tool in result.data:
            buttons.append([InlineKeyboardButton(tool, callback_data=f"od_tool_{tool}")])
        
        msg = await update.message.reply_text(
            "请选择下载工具:",
            reply_markup=InlineKeyboardMarkup(buttons)
        )
        
        DOWNLOAD_CACHE[chat_id] = {
            "step": STEP_SELECTING_TOOL,
            "message_id": msg.message_id,
            "tool": None,
            "path": None,
            "root_path": None,
            "urls": []
        }
        
    except Exception as e:
        logger.error(f"获取下载工具失败: {e}")
        await update.message.reply_text(f"获取下载工具失败: {e}")


async def od_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理所有回调"""
    query = update.callback_query
    await query.answer()
    
    chat_id = query.message.chat.id
    data = query.data
    
    if chat_id not in DOWNLOAD_CACHE:
        await query.message.edit_text("会话已过期，请重新输入 /od")
        return
    
    cache = DOWNLOAD_CACHE[chat_id]
    step = cache.get("step", STEP_SELECTING_TOOL)
    
    # 选择工具
    if step == STEP_SELECTING_TOOL and data.startswith("od_tool_"):
        tool = data.replace("od_tool_", "")
        cache["tool"] = tool
        cache["step"] = STEP_BROWSING_PATH
        
        await show_path_browser(query, cache, "/")
        return
    
    # 浏览目录
    if step == STEP_BROWSING_PATH:
        # 确认路径
        if data == "od_confirm_path":
            cache["step"] = STEP_ENTERING_URL
            path = cache.get("path", "/")
            await query.message.edit_text(
                f"已选择路径: {path}\n\n请输入下载链接 (支持 HTTP/magnet/bt):"
            )
            return
        
        await handle_path_browse(query, cache, data)
        return
    
    # 确认下载
    if data == "od_confirm":
        tool = cache.get("tool", "未知")
        path = cache.get("path", "/")
        url = cache.get("urls", [""])[0]
        
        if not url:
            await query.message.edit_text("请输入下载链接")
            return
        
        try:
            result = await openlist.add_offline_download(
                urls=[url],
                tool=tool,
                path=path,
                delete_policy="0"
            )
            
            if result.code == 200:
                await query.message.edit_text(
                    f"✅ 下载任务已创建!\n\n"
                    f"工具: {tool}\n"
                    f"路径: {path}\n"
                    f"链接: {url}"
                )
            else:
                await query.message.edit_text(f"❌ 创建失败: {result.message}")
                
        except Exception as e:
            logger.error(f"创建下载任务失败: {e}")
            await query.message.edit_text(f"❌ 创建失败: {e}")
        
        if chat_id in DOWNLOAD_CACHE:
            del DOWNLOAD_CACHE[chat_id]
        return
    
    # 取消
    if data == "od_cancel":
        if chat_id in DOWNLOAD_CACHE:
            del DOWNLOAD_CACHE[chat_id]
        await query.message.edit_text("❌ 已取消")
        return


async def show_path_browser(query, cache, path):
    """显示路径浏览器"""
    try:
        result = await openlist.fs_list(path)
        
        content = []
        if result.data:
            if isinstance(result.data, dict):
                content = result.data.get("content", [])
            elif isinstance(result.data, list):
                content = result.data
        
        dirs = []
        files = []
        for item in content:
            if isinstance(item, dict):
                is_dir = item.get("is_dir", False)
                name = item.get("name", "")
            else:
                is_dir = getattr(item, "is_dir", False)
                name = getattr(item, "name", "")
            
            if is_dir:
                dirs.append({"name": name})
            else:
                files.append({"name": name})
        
        buttons = []

        # 添加子目录
        for d in dirs[:10]:
            name = d["name"]
            sub_path = f"{path.rstrip('/')}/{name}/"
            path_id = od_register_path(sub_path)
            buttons.append([InlineKeyboardButton(f"📁 {name}", callback_data=f"od_cd_{path_id}")])

        # 添加文件（只显示前几个作为参考）
        for f in files[:5]:
            name = f["name"]
            buttons.append([InlineKeyboardButton(f"📄 {name}", callback_data=f"od_file_{name}")])

        # 添加确认按钮
        buttons.append([InlineKeyboardButton("✅ 确认此路径", callback_data="od_confirm_path")])

        # 添加返回按钮
        if path != "/":
            parent_path = "/".join(path.rstrip("/").split("/")[:-1]) or "/"
            parent_id = od_register_path(parent_path)
            buttons.append([InlineKeyboardButton("⬅️ 返回上一级", callback_data=f"od_cd_{parent_id}")])
        
        cache["path"] = path
        
        await query.message.edit_text(
            f"📁 选择存储路径: `{path}`\n\n点击目录进入，点击确认此路径按钮完成选择",
            reply_markup=InlineKeyboardMarkup(buttons),
            parse_mode="Markdown"
        )
        
    except Exception as e:
        logger.error(f"获取目录失败: {e}")
        await query.message.edit_text(f"获取目录失败: {e}")


async def handle_path_browse(query, cache, data):
    """处理路径浏览回调"""
    if data.startswith("od_cd_"):
        path_id = data.replace("od_cd_", "")
        path = od_get_path(path_id)
        if not path:
            await query.answer("路径已过期，请重新选择", show_alert=True)
            return
        await show_path_browser(query, cache, path)
        return
    
    if data.startswith("od_file_"):
        file_name = data.replace("od_file_", "")
        await query.answer(f"文件: {file_name}", show_alert=True)
        return


async def od_message(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理文本输入"""
    if not update.message:
        return
    
    chat_id = update.effective_chat.id
    
    if chat_id not in DOWNLOAD_CACHE:
        return
    
    cache = DOWNLOAD_CACHE[chat_id]
    step = cache.get("step", "")
    text = update.message.text.strip()
    
    # 输入下载链接
    if step == STEP_ENTERING_URL:
        cache["urls"] = [text]
        cache["step"] = STEP_CONFIRMING
        
        tool = cache.get("tool", "未知")
        path = cache.get("path", "/")
        
        await update.message.reply_text(
            f"📥 确认下载\n\n"
            f"工具: {tool}\n"
            f"路径: {path}\n"
            f"链接: {text}\n",
            reply_markup=InlineKeyboardMarkup([
                [InlineKeyboardButton("✅ 确认下载", callback_data="od_confirm")],
                [InlineKeyboardButton("❌ 取消", callback_data="od_cancel")]
            ])
        )


ODS_CALLBACK_DATA = {}


def extract_file_info(name: str) -> tuple:
    """从任务名提取完整文件名和路径"""
    full_name = name
    filename = "未知"
    target_path = "/"
    
    if "download " in name:
        parts = name.split("download ")[1].split(" to (")
        if len(parts) >= 2:
            filename = parts[0]
            target_path = parts[1].rstrip(")")
    
    return filename, target_path


async def ods_command(update: Update, context: ContextTypes.DEFAULT_TYPE, page: int = 1):
    """查看下载状态"""
    if not await check_is_admin(update):
        return
    
    chat_id = update.effective_chat.id
    
    try:
        await openlist.login()
        
        undone = await openlist.get_offline_download_undone_task()
        done = await openlist.get_offline_download_done_task()
        
        # 合并所有任务
        all_tasks = []
        if undone.data:
            for task in undone.data:
                task["_type"] = "undone"
                all_tasks.append(task)
        if done.data:
            for task in done.data:
                task["_type"] = "done"
                all_tasks.append(task)
        
        total = len(all_tasks)
        total_pages = max(1, (total + 9) // 10)
        page = max(1, min(page, total_pages))
        
        start_idx = (page - 1) * 10
        end_idx = start_idx + 10
        page_tasks = all_tasks[start_idx:end_idx]
        
        # 保存任务信息用于回调
        ODS_CALLBACK_DATA[chat_id] = {
            "tasks": all_tasks,
            "page": page,
            "total_pages": total_pages
        }
        
        # 统计
        undone_count = len(undone.data) if undone.data else 0
        done_count = len(done.data) if done.data else 0
        
        text = f"📥 离线下载状态\n"
        text += f"━━━━━━━━━━━━━━━━━━\n"
        text += f"⏳ 进行中: {undone_count} 个\n"
        text += f"✅ 已完成: {done_count} 个\n"
        text += f"━━━━━━━━━━━━━━━━━━\n"
        text += f"共 {total} 个任务 (第 {page}/{total_pages} 页)\n\n"
        
        buttons = []
        
        for task in page_tasks:
            task_id = task.get("id", "")
            name_full, target_path = extract_file_info(task.get("name", ""))
            task_type = task.get("_type", "unknown")
            
            if task_type == "undone":
                progress = task.get("progress", 0)
                btn_text = f"⏳ {name_full} ({progress}%)"
                if len(btn_text) > 40:
                    btn_text = btn_text[:37] + "..."
                buttons.append([
                    InlineKeyboardButton(btn_text, callback_data=f"ods_detail_{task_id}")
                ])
            else:
                size = format_size(task.get("total_bytes", 0))
                btn_text = f"✅ {name_full} ({size})"
                if len(btn_text) > 40:
                    btn_text = btn_text[:37] + "..."
                buttons.append([
                    InlineKeyboardButton(btn_text, callback_data=f"ods_detail_{task_id}")
                ])
        
        # 分页按钮
        page_buttons = []
        if page > 1:
            page_buttons.append(InlineKeyboardButton("⬅️ 上一页", callback_data=f"ods_page_{page-1}"))
        if page < total_pages:
            page_buttons.append(InlineKeyboardButton("➡️ 下一页", callback_data=f"ods_page_{page+1}"))
        if page_buttons:
            buttons.append(page_buttons)
        
        # 添加关闭按钮
        buttons.append([InlineKeyboardButton("❌ 关闭", callback_data="ods_close")])
        
        # 确定使用哪个方法发送消息
        send_message = None
        if update.message:
            send_message = update.message.reply_text
        elif update.callback_query and update.callback_query.message:
            send_message = update.callback_query.message.reply_text
        
        if send_message:
            await send_message(
                text,
                reply_markup=InlineKeyboardMarkup(buttons),
                disable_web_page_preview=True
            )
        
    except Exception as e:
        logger.error(f"获取下载状态失败: {e}")
        if update.message:
            await update.message.reply_text(f"获取状态失败: {e}")
        elif update.callback_query and update.callback_query.message:
            await update.callback_query.message.reply_text(f"获取状态失败: {e}")


async def ods_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理 ods 回调"""
    query = update.callback_query
    await query.answer()
    
    chat_id = query.message.chat.id
    data = query.data
    
    logger.info(f"ods_callback: {data}")
    
    # 分页
    if data.startswith("ods_page_"):
        page = int(data.replace("ods_page_", ""))
        await ods_command(update, context, page=page)
        return
    
    # 任务详情
    if data.startswith("ods_detail_"):
        task_id = data.replace("ods_detail_", "")
        
        # 查找任务
        ods_data = ODS_CALLBACK_DATA.get(chat_id, {})
        tasks = ods_data.get("tasks", [])
        
        task = None
        for t in tasks:
            if t.get("id") == task_id:
                task = t
                break
        
        if not task:
            await query.message.edit_text("任务已失效，请重新输入 /ods")
            return
        
        name, target_path = extract_file_info(task.get("name", ""))
        task_type = task.get("_type", "unknown")
        
        # 获取目标路径
        full_name = task.get("name", "")
        target_path = "/"
        if " to (" in full_name:
            target_path = full_name.split(" to (")[1].rstrip(")")
        
        if task_type == "undone":
            progress = task.get("progress", 0)
            status = task.get("status", "")
            text = f"⏳ 任务详情\n\n文件名: {name}\n进度: {progress}%\n状态: {status}\n路径: {target_path}\n"
        else:
            size = format_size(task.get("total_bytes", 0))
            # 获取下载链接
            try:
                result = await openlist.fs_get(target_path)
                raw_url = ""
                if result.data:
                    if isinstance(result.data, dict):
                        raw_url = result.data.get("raw_url", "")
                
                web_url = bot_cfg.openlist_web.rstrip("/")
                full_url = f"{web_url}{target_path}"
                
                text = f"✅ 任务详情\n\n文件名: {name}\n大小: {size}\n路径: {target_path}\n\n直链: {raw_url}\n打开链接: {full_url}"
            except Exception as e:
                logger.error(f"获取下载链接失败: {e}")
                text = f"✅ 任务详情\n\n文件名: {name}\n大小: {size}\n路径: {target_path}\n\n(获取链接失败)"
        
        # 根据任务类型添加按钮
        current_page = ods_data.get("page", 1)
        if task_type == "undone":
            buttons = [
                [InlineKeyboardButton("❌ 取消任务", callback_data=f"ods_cancel_{task_id}")],
                [InlineKeyboardButton("⬅️ 返回列表", callback_data=f"ods_page_{current_page}")]
            ]
        else:
            buttons = [
                [InlineKeyboardButton("⬅️ 返回列表", callback_data=f"ods_page_{current_page}")]
            ]
        
        await query.message.edit_text(text, reply_markup=InlineKeyboardMarkup(buttons), disable_web_page_preview=True)
        return
    
    # 取消任务
    if data.startswith("ods_cancel_"):
        task_id = data.replace("ods_cancel_", "")
        await query.answer("取消功能开发中", show_alert=True)
        return
    
    # 关闭
    if data == "ods_close":
        await query.message.delete()
        return


def register_handlers(app: Application):
    app.add_handler(CommandHandler("od", od_command))
    app.add_handler(CommandHandler("ods", ods_command))
    app.add_handler(CallbackQueryHandler(od_callback, pattern=r"^od_"))
    app.add_handler(CallbackQueryHandler(ods_callback, pattern=r"^ods_"))
    app.add_handler(MessageHandler(filters.TEXT & ~filters.COMMAND, od_message))


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
