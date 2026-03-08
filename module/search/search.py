# -*- coding: UTF-8 -*-
import asyncio
import math
import urllib.parse
from typing import Optional

from telegram import Update, InlineKeyboardButton, InlineKeyboardMarkup
from telegram.ext import (
    Application,
    CommandHandler,
    CallbackQueryHandler,
    ContextTypes,
)

from api.pansou.pansou_api import pansou, PanSouResult
from config.config import bot_cfg
from tools.filters import is_admin, is_member

PAGE: dict[str, "Page"] = {}

# 结果缓存，用于下载按钮回调（key: "{cmid}_{global_index}"）
S_RESULT_CACHE: dict[str, PanSouResult] = {}

PAN_TYPE_EMOJI = {
    "baidu": "🔵百度",
    "aliyun": "🟢阿里",
    "quark": "🟣夸克",
    "tianyi": "🔴天翼",
    "115": "🟠115",
    "pikpak": "⚡PikPak",
    "xunlei": "⚙️迅雷",
    "123": "🔶123",
    "magnet": "🧲磁力",
    "ed2k": "📎电驴",
    "uc": "🌐UC",
}


def _md_escape(text: str) -> str:
    """转义 Telegram Markdown v1 特殊字符（用于动态内容）"""
    for ch in ("_", "*", "[", "`"):
        text = text.replace(ch, f"\\{ch}")
    return text


def build_result_item_sync(count: int, item: PanSouResult) -> str:
    file_name = item.name
    file_url = item.url
    file_size = item.size
    pan_type = item.pan_type
    
    emoji = PAN_TYPE_EMOJI.get(pan_type, "📦")

    password = item.password
    if not password and "pwd=" in file_url:
        try:
            password = urllib.parse.parse_qs(urllib.parse.urlparse(file_url).query).get("pwd", [""])[0]
        except Exception:
            password = ""

    pwd_text = f" | 密码: `{password}`" if password else ""
    
    clean_url = file_url.split("#")[0] if file_url else ""
    url_text = f"\n🔗 {_md_escape(clean_url)}" if clean_url else ""

    return f"{count}. {emoji} `{file_name}`\n{file_size}{pwd_text}{url_text}\n\n"


class Page:
    PER_PAGE = 5

    def __init__(self, results: list[PanSouResult], keyword: str = "", cmid: str = ""):
        self.all_results = results
        self.keyword = keyword
        self.cmid = cmid
        self.filter_type: Optional[str] = None
        self.index = 0
        self.per_page = self.PER_PAGE
        self._update_filtered_results()

    def _update_filtered_results(self):
        if self.filter_type:
            self.filtered_results = [r for r in self.all_results if r.pan_type == self.filter_type]
        else:
            self.filtered_results = self.all_results
        self.page_count = math.ceil(len(self.filtered_results) / self.per_page) if self.filtered_results else 1

    @property
    def results(self) -> list[PanSouResult]:
        return self.filtered_results

    def set_filter(self, pan_type: Optional[str]):
        self.filter_type = pan_type
        self.index = 0
        self._update_filtered_results()

    def get_text_with_info(self) -> str:
        text, total, current = self.now_page()
        filter_name = ""
        if self.filter_type:
            filter_name = f" | {PAN_TYPE_EMOJI.get(self.filter_type, self.filter_type)}"
        filter_info = f"🔍 {self.keyword} | {total} 条结果{filter_name}\n\n"
        return filter_info + text

    def now_page(self) -> tuple[str, int, int]:
        start = self.index * self.per_page
        end = start + self.per_page
        items = self.filtered_results[start:end]
        
        text_parts = []
        for i, item in enumerate(items):
            count = start + i + 1
            text_parts.append(build_result_item_sync(count, item))
        
        return "".join(text_parts), len(self.filtered_results), self.index + 1

    def next_page(self) -> str:
        if self.index < self.page_count - 1:
            self.index += 1
        return self.now_page()[0]

    def previous_page(self) -> str:
        if self.index > 0:
            self.index -= 1
        return self.now_page()[0]

    @property
    def btn(self) -> list:
        buttons = []

        # 为当前页的磁力/电驴结果添加下载按钮
        if self.cmid:
            start = self.index * self.per_page
            items = self.filtered_results[start:start + self.per_page]
            dl_row = []
            for i, item in enumerate(items):
                if item.pan_type in ("magnet", "ed2k") and item.url:
                    global_idx = self.all_results.index(item)
                    count = start + i + 1
                    dl_row.append(InlineKeyboardButton(
                        f"📥#{count}",
                        callback_data=f"s_dl_{self.cmid}_{global_idx}"
                    ))
            for j in range(0, len(dl_row), 3):
                buttons.append(dl_row[j:j + 3])

        pan_types = list(set(r.pan_type for r in self.all_results))
        pan_types.sort()

        filter_row = [InlineKeyboardButton("全部", callback_data="search_filter_all")]
        for pt in pan_types[:5]:
            emoji = PAN_TYPE_EMOJI.get(pt, "📦")
            filter_row.append(InlineKeyboardButton(emoji, callback_data=f"search_filter_{pt}"))
        buttons.append(filter_row)

        if len(pan_types) > 5:
            row = []
            for pt in pan_types[5:10]:
                emoji = PAN_TYPE_EMOJI.get(pt, "📦")
                row.append(InlineKeyboardButton(emoji, callback_data=f"search_filter_{pt}"))
            if row:
                buttons.append(row)

        nav_buttons = [
            InlineKeyboardButton("⬆️ 上一页", callback_data="search_previous_page"),
            InlineKeyboardButton(f"{self.index + 1}/{self.page_count}", callback_data="search_pages"),
            InlineKeyboardButton("⬇️ 下一页", callback_data="search_next_page"),
        ]
        buttons.append(nav_buttons)

        return buttons


async def s_command(update: Update, context: ContextTypes.DEFAULT_TYPE):
    if not update.message:
        return
    
    chat = update.effective_chat
    if chat.type != "private" and bot_cfg.member:
        if chat.id not in bot_cfg.member:
            return
    
    k = " ".join(context.args)
    if not k:
        return await update.message.reply_text("请加上文件名，例：`/s 巧克力`", parse_mode="Markdown")
    msg = await update.message.reply_text("搜索中...")

    try:
        results = await pansou.search(k)
    except Exception as e:
        return await msg.edit_text(f"搜索失败: {str(e)}")

    if not results:
        return await msg.edit_text("未搜索到文件，换个关键词试试吧")

    text, button = await build_result(results, msg, k)
    await msg.edit_text(text=text, reply_markup=InlineKeyboardMarkup(button), parse_mode="Markdown", disable_web_page_preview=True)


async def build_result(content: list[PanSouResult], msg, keyword: str):
    chat_id = msg.chat.id
    message_id = msg.message_id
    cmid = f"{chat_id}|{message_id}"

    # 清理旧缓存，填充新结果
    for k in [k for k in S_RESULT_CACHE if k.startswith(f"{cmid}_")]:
        del S_RESULT_CACHE[k]
    for i, item in enumerate(content):
        S_RESULT_CACHE[f"{cmid}_{i}"] = item

    page = Page(content, keyword, cmid)
    PAGE[cmid] = page

    text = page.get_text_with_info()

    return text, page.btn


async def build_result_item(count: int, item: PanSouResult) -> str:
    return build_result_item_sync(count, item)


async def search_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    query = update.callback_query
    await query.answer()
    
    data = query.data
    msg = query.message
    cmid = f"{msg.chat.id}|{msg.message_id}"
    page = PAGE.get(cmid)
    
    if not page:
        try:
            await query.answer("搜索结果已过期，请重新搜索", show_alert=True)
        except Exception:
            pass
        return
    
    if data == "search_next_page":
        page.next_page()
        text = page.get_text_with_info()
    elif data == "search_previous_page":
        page.previous_page()
        text = page.get_text_with_info()
    elif data == "search_filter_all":
        page.set_filter(None)
        text = page.get_text_with_info()
    elif data.startswith("search_filter_"):
        pan_type = data.replace("search_filter_", "")
        page.set_filter(pan_type)
        text = page.get_text_with_info()
    elif data.startswith("s_dl_"):
        # 解析 s_dl_{cmid}_{global_idx}，cmid 含 "|" 故从右截取最后一段
        rest = data[5:]
        last_sep = rest.rfind("_")
        cmid_part = rest[:last_sep]
        try:
            global_idx = int(rest[last_sep + 1:])
        except ValueError:
            return

        item = S_RESULT_CACHE.get(f"{cmid_part}_{global_idx}")
        if not item or not item.url:
            await query.answer("链接已过期，请重新搜索", show_alert=True)
            return

        try:
            from config.config import reload_od_cfg
            cfg = reload_od_cfg()
            dl_path = cfg.download_path if cfg and cfg.download_path else "/"
            dl_tool = cfg.download_tool if cfg and cfg.download_tool else "qbittorrent"
        except Exception:
            dl_path = "/"
            dl_tool = "qbittorrent"

        file_name = item.name[:30] + "..." if len(item.name) > 30 else item.name
        from module.torrent_search.torrent_search import DOWNLOAD_INFO, build_download_confirm_message
        DOWNLOAD_INFO[msg.chat.id] = {
            "result": item,
            "index": global_idx,
            "cmid": cmid_part,
            "path": dl_path,
            "tool": dl_tool,
            "url": item.url,
            "file_name": file_name,
        }
        text_confirm, keyboard = await build_download_confirm_message(msg.chat.id, DOWNLOAD_INFO[msg.chat.id])
        await msg.edit_text(text_confirm, reply_markup=keyboard, parse_mode="Markdown")
        return
    else:
        return

    try:
        await msg.edit_text(text, reply_markup=InlineKeyboardMarkup(page.btn), parse_mode="Markdown", disable_web_page_preview=True)
    except Exception:
        pass


def register_handlers(app: Application):
    app.add_handler(CommandHandler("s", s_command))
    app.add_handler(CallbackQueryHandler(search_callback, pattern=r"^(search|s_dl_)"))
