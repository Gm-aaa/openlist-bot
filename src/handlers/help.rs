use std::sync::Arc;
use teloxide::prelude::*;
use crate::BotContext;
use crate::utils::is_admin;

pub async fn handle_start(bot: Bot, msg: Message) -> ResponseResult<()> {
    let text = "📚 OpenList Bot 命令帮助\n\n\
                🔍 搜索\n\
                /search <关键词> - 搜索网盘文件（支持按网盘类型筛选）\n\n\
                📂 存储浏览\n\
                /browse - 浏览 OpenList 存储文件\n\n\
                📥 离线下载\n\
                /download - 离线下载与设置（新建任务/查看状态/配置）\n\n\
                🔄 刷新\n\
                /refresh - 刷新 OpenList 文件缓存\n\n\
                💡 使用说明：\n\
                • /search 搜索网盘文件，点击按钮可筛选网盘类型\n\
                • /browse 选择存储后可以浏览目录，支持删除、新建文件夹、上传文件\n\
                • /download 选择新建任务、查看状态或配置下载设置\n\
                • /refresh 刷新 OpenList 文件缓存";
    bot.send_message(msg.chat.id, text).await?;
    Ok(())
}

pub async fn handle_help(bot: Bot, msg: Message, ctx: Arc<BotContext>) -> ResponseResult<()> {
    let user_id = msg.from().map_or(0, |u| u.id.0 as i64);
    let cfg = ctx.config.read().await;
    if !is_admin(user_id, &cfg) {
        return Ok(());
    }

    let text = "📚 OpenList Bot 命令帮助\n\n\
                🔍 搜索\n\
                /search <关键词> - 搜索网盘文件（支持按网盘类型筛选）\n\n\
                📂 存储浏览\n\
                /browse - 浏览 OpenList 存储文件\n\n\
                📥 离线下载\n\
                /download - 离线下载与设置（新建任务/查看状态/配置）\n\n\
                🔄 刷新\n\
                /refresh - 刷新 OpenList 文件缓存\n\n\
                💡 使用说明：\n\
                • /search 搜索网盘文件，点击按钮可筛选网盘类型\n\
                • /browse 选择存储后可以浏览目录，支持删除、新建文件夹、上传文件\n\
                • /download 选择新建任务、查看状态或配置下载设置\n\
                • /refresh 刷新 OpenList 文件缓存";
    bot.send_message(msg.chat.id, text).await?;
    Ok(())
}
