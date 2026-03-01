from telegram import Update
from telegram.ext import filters

from config.config import bot_cfg


class IsAdmin(filters.BaseFilter):
    async def __call__(self, update: Update) -> bool:
        user = update.effective_user
        if not user:
            return False
        return user.id == bot_cfg.admin


class IsMember(filters.BaseFilter):
    async def __call__(self, update: Update) -> bool:
        chat = update.effective_chat
        if not chat:
            return False
        if not bot_cfg.member:
            return True
        return chat.id in bot_cfg.member


is_admin = IsAdmin()
is_member = IsMember()


async def check_is_admin(update_or_user_id) -> bool:
    """检查是否为管理员，可接受 Update 对象或 user_id"""
    if isinstance(update_or_user_id, int):
        return update_or_user_id == bot_cfg.admin
    user = update_or_user_id.effective_user
    if not user:
        return False
    return user.id == bot_cfg.admin
