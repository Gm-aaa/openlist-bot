# -*- coding: UTF-8 -*-
import math

from loguru import logger
from telegram import Update, InlineKeyboardButton, InlineKeyboardMarkup
from telegram.ext import (
    Application,
    CommandHandler,
    CallbackQueryHandler,
    ContextTypes,
)

from api.prowlarr.prowlarr_api import get_prowlarr, ProwlarrResult
from config.config import bot_cfg
from tools.torrent_cache import TorrentCache
from api.openlist.openlist_api import openlist


PAGE: dict[str, "TorrentPage"] = {}
RESULT_INDEXER: dict[str, ProwlarrResult] = {}
INDEXERS: dict[int, str] = {}
KEYWORD_CACHE: dict[int, str] = {}

# 下载信息缓存（用于路径/工具选择）
DOWNLOAD_INFO: dict[int, dict] = {}

# 路径浏览缓存
SB_PATH_CACHE: dict[int, dict] = {}
SB_PATH_COUNTER = 0
SB_PATH_MAP = {}


def sb_register_path(path: str) -> str:
    """注册路径并返回短ID"""
    global SB_PATH_COUNTER
    SB_PATH_COUNTER += 1
    path_id = str(SB_PATH_COUNTER)
    SB_PATH_MAP[path_id] = path
    return path_id


def sb_get_path(path_id: str) -> str:
    """根据ID获取路径"""
    return SB_PATH_MAP.get(path_id, "")


async def build_download_confirm_message(chat_id: int, info: dict) -> tuple[str, InlineKeyboardMarkup]:
    """构建下载确认页面"""
    keyboard = [
        [InlineKeyboardButton(f"📂 路径: {info['path']}", callback_data="sb_path_select")],
        [InlineKeyboardButton(f"🔧 工具: {info['tool']}", callback_data="sb_tool_select")],
        [InlineKeyboardButton("✅ 确认下载", callback_data=f"tb_add_{info['index']}_{info['cmid']}")],
    ]
    
    text = (
        f"📥 添加到离线下载\n\n"
        f"📄 文件: `{info['file_name']}`\n"
        f"📂 路径: {info['path']}\n"
        f"🔧 工具: {info['tool']}\n"
        f"🔗 链接: 磁力链接"
    )
    return text, InlineKeyboardMarkup(keyboard)


class TorrentPage:
    def __init__(self, results: list[ProwlarrResult], keyword: str = ""):
        self.all_results = results
        self.keyword = keyword
        self.index = 0
        self.per_page = 5
        self.page_count = math.ceil(len(results) / self.per_page) if results else 1

    @property
    def results(self) -> list[ProwlarrResult]:
        start = self.index * self.per_page
        end = start + self.per_page
        return self.all_results[start:end]

    def next_page(self):
        if self.index < self.page_count - 1:
            self.index += 1

    def previous_page(self):
        if self.index > 0:
            self.index -= 1

    def now_page_text(self) -> tuple[str, list]:
        text_parts = []
        buttons = []
        items = self.results
        start = self.index * self.per_page
        
        for i, item in enumerate(items):
            count = start + i + 1
            text, item_btns = build_result_item(count, item, start + i)
            text_parts.append(text)
            buttons.extend(item_btns)
        
        header = f"🔍 关键词: {self.keyword} | 结果: {len(self.all_results)} 条\n\n"
        text = header + "".join(text_parts)
        
        nav_buttons = [
            InlineKeyboardButton("⬆️", callback_data="torrent_previous"),
            InlineKeyboardButton(f"{self.index + 1}/{self.page_count}", callback_data="torrent_page"),
            InlineKeyboardButton("⬇️", callback_data="torrent_next"),
        ]
        buttons.append(nav_buttons)
        
        return text, buttons


def build_result_item(count: int, item: ProwlarrResult, index: int) -> tuple[str, list]:
    size_str = item.size_str
    seeds = f"✅{item.seeders}" if item.seeders else ""
    leeches = f"❌{item.leechers}" if item.leechers else ""
    peer_info = f" {seeds} {leeches}" if seeds or leeches else ""
    indexer = f"📡{item.indexer}"
    
    text = f"{count}. `{item.title}`\n{size_str}{peer_info} | {indexer}\n"
    
    buttons = []
    row1 = []
    row1.append(InlineKeyboardButton("🧲磁力", callback_data=f"torrent_magnet_{index}"))
    row1.append(InlineKeyboardButton("📥下载", callback_data=f"torrent_add_{index}"))
    buttons.append(row1)
    
    return text, buttons


async def build_path_browser(path: str, chat_id: int):
    """构建路径浏览界面"""
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
            path_id = sb_register_path(sub_path)
            buttons.append([InlineKeyboardButton(f"📁 {name}", callback_data=f"sb_cd_{path_id}")])
        
        # 添加文件（只显示前几个作为参考）
        for f in files[:5]:
            name = f["name"]
            buttons.append([InlineKeyboardButton(f"📄 {name}", callback_data=f"sb_file_{name}")])
        
        # 添加确认按钮
        buttons.append([InlineKeyboardButton("✅ 确认此路径", callback_data="sb_confirm_path")])
        
        # 添加返回按钮
        if path != "/":
            parent_path = "/".join(path.rstrip("/").split("/")[:-1]) or "/"
            parent_path_id = sb_register_path(parent_path)
            buttons.append([InlineKeyboardButton("⬅️ 返回上一级", callback_data=f"sb_cd_{parent_path_id}")])
        
        # 保存当前路径
        SB_PATH_CACHE[chat_id] = {
            "path": path,
            "root_path": path
        }
        
        return f"📂 选择下载路径: `{path}`\n\n点击目录进入，点击确认此路径按钮完成选择", buttons
    except Exception as e:
        logger.error(f"获取目录失败: {e}")
        return f"获取目录失败: {e}", []


async def load_indexers():
    """加载索引器列表"""
    global INDEXERS
    try:
        prowlarr = get_prowlarr()
        indexers = await prowlarr.get_indexers()
        INDEXERS.clear()
        for idx in indexers:
            INDEXERS[idx.get("id")] = idx.get("name", "")
        logger.info(f"加载索引器: {len(INDEXERS)} 个")
    except Exception as e:
        logger.error(f"加载索引器失败: {e}")


async def sb_command(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """搜索种子/磁力链接"""
    if not update.message:
        return
    
    chat = update.effective_chat
    if chat.type != "private" and bot_cfg.member:
        if chat.id not in bot_cfg.member:
            return
    
    args = context.args
    if not args:
        return await update.message.reply_text("请加上关键词，例：`/sb 电影名称`")
    
    keyword = " ".join(args)
    if not keyword:
        return await update.message.reply_text("请加上关键词，例：`/sb 电影名称`")
    
    await load_indexers()

    if not INDEXERS:
        return await update.message.reply_text("无法获取索引器列表，请检查 Prowlarr 配置")

    # 将关键词存入缓存，callback_data 只携带 chat_id，避免超过 64 字节限制
    KEYWORD_CACHE[chat.id] = keyword

    keyboard = []
    row = []
    for idx_id, idx_name in INDEXERS.items():
        row.append(InlineKeyboardButton(
            idx_name[:12],
            callback_data=f"sb_indexer_{idx_id}_{chat.id}"
        ))
        if len(row) == 2:
            keyboard.append(row)
            row = []
    if row:
        keyboard.append(row)

    keyboard.append([InlineKeyboardButton("🔍 全部索引器", callback_data=f"sb_indexer_all_{chat.id}")])

    await update.message.reply_text(
        f"🔍 选择要使用的索引器:\n关键词: {keyword}",
        reply_markup=InlineKeyboardMarkup(keyboard)
    )


async def search_with_indexer(update: Update, context: ContextTypes.DEFAULT_TYPE, indexer_id: int, keyword: str, msg_text: str):
    """使用指定索引器搜索"""
    # 使用原始消息的 ID，而不是新消息
    original_msg = update.callback_query.message
    chat_id = update.effective_chat.id
    original_msg_id = original_msg.message_id
    cmid = f"{chat_id}|{original_msg_id}"
    
    # 先编辑原消息为搜索中
    await original_msg.edit_text("🔎 搜索中...")
    
    try:
        prowlarr = get_prowlarr()
        if indexer_id == -1:
            results = await prowlarr.search(keyword, limit=20)
            indexer_name = "全部"
        else:
            results = await prowlarr.search(keyword, indexer_ids=[indexer_id], limit=20)
            indexer_name = INDEXERS.get(indexer_id, str(indexer_id))
    except Exception as e:
        return await original_msg.edit_text(f"搜索失败: {str(e)}")
    
    if not results:
        return await original_msg.edit_text("未搜索到结果")
    
    keys_to_delete = [k for k in RESULT_INDEXER if k.startswith(f"{cmid}_")]
    for k in keys_to_delete:
        del RESULT_INDEXER[k]
    for i, r in enumerate(results):
        RESULT_INDEXER[f"{cmid}_{i}"] = r
    
    page = TorrentPage(results, f"{keyword} | 📡{indexer_name}")
    PAGE[cmid] = page
    
    text, buttons = page.now_page_text()
    await original_msg.edit_text(text=text, reply_markup=InlineKeyboardMarkup(buttons))


async def torrent_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    query = update.callback_query
    
    try:
        await query.answer()
    except Exception:
        pass
    
    data = query.data
    msg = query.message
    
    if data.startswith("sb_indexer_"):
        parts = data.replace("sb_indexer_", "").split("_", 1)
        if len(parts) != 2:
            return

        indexer_part = parts[0]
        try:
            source_chat_id = int(parts[1])
        except ValueError:
            return

        keyword = KEYWORD_CACHE.get(source_chat_id, "")
        if not keyword:
            try:
                await query.answer("会话已过期，请重新搜索", show_alert=True)
            except Exception:
                pass
            return

        if indexer_part == "all":
            indexer_id = -1
        else:
            try:
                indexer_id = int(indexer_part)
            except ValueError:
                return

        await search_with_indexer(update, context, indexer_id, keyword, query.message.text)
        return
    
    cmid = f"{msg.chat.id}|{msg.message_id}"
    page = PAGE.get(cmid)
    
    if not page:
        try:
            await query.answer("搜索结果已过期，请重新搜索", show_alert=True)
        except Exception:
            pass
        return
    
    if data == "torrent_next":
        page.next_page()
        text, buttons = page.now_page_text()
        await msg.edit_text(text=text, reply_markup=InlineKeyboardMarkup(buttons))
        return
    elif data == "torrent_previous":
        page.previous_page()
        text, buttons = page.now_page_text()
        await msg.edit_text(text=text, reply_markup=InlineKeyboardMarkup(buttons))
        return
    elif data.startswith("torrent_magnet_"):
        try:
            index = int(data.split("_")[-1])
        except ValueError:
            return
        result = RESULT_INDEXER.get(f"{cmid}_{index}")
        if not result:
            return
        
        try:
            await query.answer("🔄 转换中...", show_alert=False)
        except Exception:
            pass
        
        cache = TorrentCache(bot_cfg.torrent_cache_max or 10)
        
        # 检查是否已有有效的磁力链接
        magnet = None
        if result.magnet_url and result.magnet_url.startswith("magnet:"):
            magnet = result.magnet_url
        elif result.torrent_url:
            magnet = await cache.get_magnet(result.torrent_url)
        else:
            try:
                await query.answer("无可用链接", show_alert=True)
            except Exception:
                pass
            return
        
        if magnet:
            await query.message.reply_text(
                f"🧲 磁力链接:\n\n`{magnet}`\n\n👆 点击复制",
                parse_mode="Markdown"
            )
        else:
            try:
                await query.answer("转换失败", show_alert=True)
            except Exception:
                pass
        return
    elif data.startswith("torrent_add_"):
        try:
            index = int(data.split("_")[-1])
        except ValueError:
            return
        result = RESULT_INDEXER.get(f"{cmid}_{index}")
        if not result:
            try:
                await query.answer("搜索结果已过期，请重新搜索", show_alert=True)
            except Exception:
                pass
            return
        
        try:
            await query.answer("🔄 获取下载信息...", show_alert=False)
        except Exception:
            pass
        
        # 获取下载工具和路径 - 使用刷新后的配置
        try:
            from config.config import reload_od_cfg
            cfg = reload_od_cfg()
            path = cfg.download_path if cfg and cfg.download_path else "/"
            tool = cfg.download_tool if cfg and cfg.download_tool else "qbittorrent"
        except Exception:
            path = "/"
            tool = "qbittorrent"
        
        # 尝试获取有效的磁力链接
        download_url = None
        if result.magnet_url and result.magnet_url.startswith("magnet:"):
            download_url = result.magnet_url
        elif result.torrent_url:
            cache = TorrentCache(bot_cfg.torrent_cache_max or 10)
            magnet = await cache.get_magnet(result.torrent_url)
            if magnet:
                download_url = magnet
        
        if not download_url:
            try:
                await query.answer("无可用链接", show_alert=True)
            except Exception:
                pass
            return
        
        chat_id = query.message.chat.id
        
        # 显示确认按钮，包含工具和路径信息
        file_name = result.title[:30] + "..." if len(result.title) > 30 else result.title
        
        # 保存下载信息到缓存
        DOWNLOAD_INFO[chat_id] = {
            "result": result,
            "index": index,
            "cmid": cmid,
            "path": path,
            "tool": tool,
            "url": download_url,
            "file_name": file_name
        }
        
        text, keyboard = await build_download_confirm_message(chat_id, DOWNLOAD_INFO[chat_id])
        await query.message.edit_text(text, reply_markup=keyboard, parse_mode="Markdown")
        return
    
    # 选择下载路径 - 显示存储列表
    if data == "sb_path_select":
        chat_id = msg.chat.id
        if chat_id not in DOWNLOAD_INFO:
            try:
                await query.answer("会话已过期，请重新搜索", show_alert=True)
            except Exception:
                pass
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
                await query.answer("暂无可用存储", show_alert=True)
                return
            
            buttons = []
            for storage in storages:
                if storage.disabled:
                    continue
                name = storage.remark or storage.mount_path or str(storage.id)
                mount_path = storage.mount_path or "/"
                # 直接进入目录浏览
                path_id = sb_register_path(mount_path)
                buttons.append([InlineKeyboardButton(
                    f"📁 {name}",
                    callback_data=f"sb_cd_{path_id}"
                )])
            
            buttons.append([InlineKeyboardButton("❌ 取消", callback_data="sb_cancel_path")])
            
            await query.message.edit_text(
                "📂 选择存储路径:",
                reply_markup=InlineKeyboardMarkup(buttons)
            )
        except Exception as e:
            logger.error(f"获取存储列表失败: {e}")
            await query.answer(f"获取存储失败: {e}", show_alert=True)
        return
    
    # 浏览目录
    if data.startswith("sb_cd_"):
        chat_id = msg.chat.id
        if chat_id not in DOWNLOAD_INFO:
            try:
                await query.answer("会话已过期，请重新搜索", show_alert=True)
            except Exception:
                pass
            return
        
        path_id = data.replace("sb_cd_", "")
        path = sb_get_path(path_id)
        if not path:
            await query.answer("路径已过期，请重新选择", show_alert=True)
            return
        
        text, buttons = await build_path_browser(path, chat_id)
        await query.message.edit_text(
            text=text,
            reply_markup=InlineKeyboardMarkup(buttons),
            parse_mode="Markdown"
        )
        return
    
    # 确认下载路径
    if data == "sb_confirm_path":
        chat_id = msg.chat.id
        if chat_id not in DOWNLOAD_INFO:
            try:
                await query.answer("会话已过期，请重新搜索", show_alert=True)
            except Exception:
                pass
            return
        
        path_info = SB_PATH_CACHE.get(chat_id, {})
        selected_path = path_info.get("path", "/")
        DOWNLOAD_INFO[chat_id]["path"] = selected_path
        
        info = DOWNLOAD_INFO[chat_id]
        text, keyboard = await build_download_confirm_message(chat_id, info)
        await query.message.edit_text(text, reply_markup=keyboard, parse_mode="Markdown")
        return
    
    # 选择下载工具
    if data == "sb_tool_select":
        chat_id = msg.chat.id
        if chat_id not in DOWNLOAD_INFO:
            try:
                await query.answer("会话已过期，请重新搜索", show_alert=True)
            except Exception:
                pass
            return
        
        try:
            await openlist.login()
            result = await openlist.get_offline_download_tools()
            
            if not result.data or len(result.data) == 0:
                await query.answer("暂无可用下载工具", show_alert=True)
                return
            
            buttons = []
            for tool in result.data:
                buttons.append([InlineKeyboardButton(tool, callback_data=f"sb_tool_{tool}")])
            
            buttons.append([InlineKeyboardButton("❌ 取消", callback_data="sb_cancel_tool")])
            
            await query.message.edit_text(
                "🔧 选择下载工具:",
                reply_markup=InlineKeyboardMarkup(buttons)
            )
        except Exception as e:
            logger.error(f"获取下载工具失败: {e}")
            await query.answer(f"获取工具失败: {e}", show_alert=True)
        return
    
    # 处理工具选择结果
    if data.startswith("sb_tool_"):
        chat_id = msg.chat.id
        if chat_id not in DOWNLOAD_INFO:
            try:
                await query.answer("会话已过期，请重新搜索", show_alert=True)
            except Exception:
                pass
            return
        
        new_tool = data.replace("sb_tool_", "")
        DOWNLOAD_INFO[chat_id]["tool"] = new_tool
        
        info = DOWNLOAD_INFO[chat_id]
        text, keyboard = await build_download_confirm_message(chat_id, info)
        await query.message.edit_text(text, reply_markup=keyboard, parse_mode="Markdown")
        return
    
    # 取消路径选择
    if data == "sb_cancel_path" or data == "sb_cancel_tool":
        chat_id = msg.chat.id
        if chat_id not in DOWNLOAD_INFO:
            try:
                await query.answer("会话已过期，请重新搜索", show_alert=True)
            except Exception:
                pass
            return
        
        info = DOWNLOAD_INFO[chat_id]
        text, keyboard = await build_download_confirm_message(chat_id, info)
        await query.message.edit_text(text, reply_markup=keyboard, parse_mode="Markdown")
        return
    
    elif data.startswith("tb_add_"):
        # 确认下载 - 使用缓存的下载信息
        chat_id = msg.chat.id
        if chat_id not in DOWNLOAD_INFO:
            try:
                await query.answer("会话已过期，请重新搜索", show_alert=True)
            except Exception:
                pass
            return
        
        try:
            await query.answer("🔄 准备下载...", show_alert=False)
        except Exception:
            pass
        
        info = DOWNLOAD_INFO[chat_id]
        download_url = info.get("url")
        path = info.get("path", "/")
        tool = info.get("tool", "qbittorrent")
        
        # 调用 /od 的确认流程
        from module.offline_download.offline_download import start_download_with_url
        await start_download_with_url(query.message, tool, path, download_url)


def register_handlers(app: Application):
    app.add_handler(CommandHandler("sb", sb_command))
    app.add_handler(CallbackQueryHandler(torrent_callback, pattern=r"^(sb_indexer|torrent|tb_add|sb_path_select|sb_tool_select|sb_tool_|sb_cd_|sb_confirm_path|sb_file_|sb_cancel)"))
