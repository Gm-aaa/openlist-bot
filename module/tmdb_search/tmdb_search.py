# -*- coding: UTF-8 -*-
import math
from telegram import Update, InlineKeyboardButton, InlineKeyboardMarkup
from telegram.ext import (
    Application,
    CommandHandler,
    CallbackQueryHandler,
    ContextTypes,
)

from api.tmdb.tmdb_api import get_tmdb, TMDbResult
from config.config import bot_cfg
from tools.filters import check_is_admin
from loguru import logger


PAGE: dict[str, "TMDbPage"] = {}
RESULT_TMDB: dict[str, TMDbResult] = {}


class TMDbPage:
    def __init__(self, results: list[TMDbResult], keyword: str = ""):
        self.all_results = results
        self.keyword = keyword
        self.index = 0
        self.per_page = 5
        self.page_count = math.ceil(len(results) / self.per_page) if results else 1

    @property
    def results(self) -> list[TMDbResult]:
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
        
        header = f"🔍 {self.keyword} | {len(self.all_results)} 条结果\n\n"
        text = header + "".join(text_parts)

        if self.page_count > 1:
            nav_buttons = [
                InlineKeyboardButton("⬆️ 上一页", callback_data="tmdb_previous"),
                InlineKeyboardButton(f"{self.index + 1}/{self.page_count}", callback_data="tmdb_page"),
                InlineKeyboardButton("⬇️ 下一页", callback_data="tmdb_next"),
            ]
            buttons.append(nav_buttons)
        
        return text, buttons


def build_result_item(count: int, item: TMDbResult, index: int) -> tuple[str, list]:
    media_type = "🎬 电影" if item.media_type == "movie" else "📺 电视剧"
    year = item.release_date if item.release_date else "未知"
    rating = f"⭐ {item.vote_average:.1f}" if item.vote_average else ""
    
    overview = item.overview[:100] + "..." if len(item.overview) > 100 else item.overview
    if not overview:
        overview = "暂无简介"
    
    text = f"{count}. {media_type} {rating}\n"
    text += f"📌 `{item.title}`\n"
    text += f"📅 {year} | 🎯 `{item.original_title}`\n"
    text += f"📝 {overview}\n"
    
    buttons = []
    buttons.append([InlineKeyboardButton("✅ 选择", callback_data=f"tmdb_select_{index}")])
    
    return text, buttons


async def sm_command(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """TMDB 电影搜索命令"""
    if not await check_is_admin(update):
        return
    
    if not update.message:
        return
    
    if not bot_cfg.tmdb_api_key:
        await update.message.reply_text("TMDB API 未配置，请先在配置文件中设置 tmdb.tmdb_api_key")
        return
    
    args = context.args
    if not args:
        return await update.message.reply_text("请加上关键词，例：`/sm 电影名称`", parse_mode="Markdown")
    
    keyword = " ".join(args)
    if not keyword:
        return await update.message.reply_text("请加上关键词，例：`/sm 电影名称`", parse_mode="Markdown")
    
    search_msg = await update.message.reply_text("🔄 搜索中...")
    
    try:
        tmdb = get_tmdb(bot_cfg.tmdb_api_key)
        results = await tmdb.search(keyword)
        
        if not results:
            await search_msg.edit_text("未搜索到结果")
            return
        
        chat_id = update.effective_chat.id
        cmid = f"{chat_id}|{search_msg.message_id}"
        keys_to_delete = [k for k in RESULT_TMDB if k.startswith(f"{cmid}_")]
        for k in keys_to_delete:
            del RESULT_TMDB[k]
        for i, r in enumerate(results):
            RESULT_TMDB[f"{cmid}_{i}"] = r

        page = TMDbPage(results, keyword)
        PAGE[cmid] = page

        text, buttons = page.now_page_text()
        await search_msg.edit_text(text=text, reply_markup=InlineKeyboardMarkup(buttons), parse_mode="Markdown")
    except Exception as e:
        logger.error(f"TMDB 搜索失败: {e}")
        await search_msg.edit_text(f"搜索失败: {e}")


async def tmdb_callback(update: Update, context: ContextTypes.DEFAULT_TYPE):
    """处理 TMDB 回调"""
    query = update.callback_query
    
    try:
        await query.answer()
    except Exception:
        pass
    
    data = query.data
    msg = query.message
    chat_id = msg.chat.id
    cmid = f"{chat_id}|{msg.message_id}"

    page = PAGE.get(cmid)
    if not page:
        try:
            await query.answer("会话已过期，请重新搜索", show_alert=True)
        except Exception:
            pass
        return

    if data == "tmdb_next":
        page.next_page()
        text, buttons = page.now_page_text()
        await msg.edit_text(text=text, reply_markup=InlineKeyboardMarkup(buttons), parse_mode="Markdown")
        return
    elif data == "tmdb_previous":
        page.previous_page()
        text, buttons = page.now_page_text()
        await msg.edit_text(text=text, reply_markup=InlineKeyboardMarkup(buttons), parse_mode="Markdown")
        return
    elif data.startswith("tmdb_select_"):
        try:
            index = int(data.split("_")[-1])
        except ValueError:
            return

        result_key = f"{cmid}_{index}"
        result = RESULT_TMDB.get(result_key)
        if not result:
            try:
                await query.answer("结果已过期，请重新搜索", show_alert=True)
            except Exception:
                pass
            return
        
        media_type = "电影" if result.media_type == "movie" else "电视剧"
        
        await query.message.edit_text(
            f"✅ 已选择: {result.title}\n"
            f"📺 类型: {media_type}\n"
            f"🎯 英文名: `{result.original_title}`\n"
            f"🔢 TMDB ID: `{result.id}`\n\n"
            f"💡 可使用英文名 `{result.original_title}` 进行 /sb 种子搜索",
            parse_mode="Markdown"
        )
        return


def register_handlers(app: Application):
    app.add_handler(CommandHandler("sm", sm_command))
    app.add_handler(CallbackQueryHandler(tmdb_callback, pattern=r"tmdb_"))
