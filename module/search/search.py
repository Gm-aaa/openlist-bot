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
    url_text = f"\n🔗 {clean_url}" if clean_url else ""

    return f"{count}. {emoji} `{file_name}`\n{file_size}{pwd_text}{url_text}\n\n"


class Page:
    PER_PAGE = 5
    
    def __init__(self, results: list[PanSouResult], keyword: str = ""):
        self.all_results = results
        self.keyword = keyword
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
        filter_info = f"🔍 关键词: {self.keyword} | 结果: {total} 条{filter_name}\n\n"
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
        pan_types = list(set(r.pan_type for r in self.all_results))
        pan_types.sort()
        
        filter_buttons = []
        row = [InlineKeyboardButton("全部", callback_data="search_filter_all")]
        for pt in pan_types[:5]:
            emoji = PAN_TYPE_EMOJI.get(pt, "📦")
            row.append(InlineKeyboardButton(emoji, callback_data=f"search_filter_{pt}"))
        filter_buttons.append(row)
        
        if len(pan_types) > 5:
            row = []
            for pt in pan_types[5:10]:
                emoji = PAN_TYPE_EMOJI.get(pt, "📦")
                row.append(InlineKeyboardButton(emoji, callback_data=f"search_filter_{pt}"))
            if row:
                filter_buttons.append(row)

        nav_buttons = [
            InlineKeyboardButton("⬆️上一页", callback_data="search_previous_page"),
            InlineKeyboardButton(f"{self.index + 1}/{self.page_count}", callback_data="search_pages"),
            InlineKeyboardButton("⬇️下一页", callback_data="search_next_page"),
        ]
        
        return filter_buttons + [nav_buttons]


async def s_command(update: Update, context: ContextTypes.DEFAULT_TYPE):
    if not update.message:
        return
    
    chat = update.effective_chat
    if chat.type != "private" and bot_cfg.member:
        if chat.id not in bot_cfg.member:
            return
    
    k = " ".join(context.args)
    if not k:
        return await update.message.reply_text("请加上文件名，例：`/s 巧克力`")
    msg = await update.message.reply_text("搜索中...")

    try:
        results = await pansou.search(k)
    except Exception as e:
        return await msg.edit_text(f"搜索失败: {str(e)}")

    if not results:
        return await msg.edit_text("未搜索到文件，换个关键词试试吧")

    text, button = await build_result(results, msg, k)
    await msg.edit_text(text=text, reply_markup=InlineKeyboardMarkup(button), disable_web_page_preview=True)


async def build_result(content: list[PanSouResult], msg, keyword: str):
    chat_id = msg.chat.id
    message_id = msg.message_id
    cmid = f"{chat_id}|{message_id}"
    
    page = Page(content, keyword)
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
        text = page.next_page()
    elif data == "search_previous_page":
        text = page.previous_page()
    elif data == "search_filter_all":
        page.set_filter(None)
        text = page.get_text_with_info()
    elif data.startswith("search_filter_"):
        pan_type = data.replace("search_filter_", "")
        page.set_filter(pan_type)
        text = page.get_text_with_info()
    else:
        return
    
    try:
        await msg.edit_text(text, reply_markup=InlineKeyboardMarkup(page.btn), disable_web_page_preview=True)
    except Exception:
        pass


def register_handlers(app: Application):
    app.add_handler(CommandHandler("s", s_command))
    app.add_handler(CallbackQueryHandler(search_callback, pattern=r"^search"))
