use std::sync::Arc;
use teloxide::prelude::*;
use tracing::info;

use crate::BotContext;
use crate::utils::is_admin;

pub async fn handle_refresh(bot: Bot, msg: Message, ctx: Arc<BotContext>) -> ResponseResult<()> {
    let chat_id = msg.chat.id;
    let user_id = msg.from().map_or(0, |u| u.id.0 as i64);

    let authorized = {
        let cfg = ctx.config.read().await;
        is_admin(user_id, &cfg)
    };
    if !authorized {
        return Ok(());
    }

    let result = run_refresh_openlist(&ctx).await;
    bot.send_message(chat_id, result).await?;

    Ok(())
}

pub async fn run_refresh_openlist(ctx: &BotContext) -> String {
    let download_path = {
        let cfg = ctx.config.read().await;
        cfg.openlist.download_path.clone()
    };

    info!("Triggering OpenList refresh for path: {}", download_path);
    match ctx.openlist.fs_list(&download_path).await {
        Ok(_) => format!("✅ OpenList 缓存已刷新\n\n路径: {}", download_path),
        Err(e) => format!("❌ 刷新失败: {}", e),
    }
}
