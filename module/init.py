from telegram.ext import Application

from module.search.search import register_handlers as register_search_handlers
from module.storage_browse.storage_browse import register_handlers as register_storage_handlers
from module.offline_download.offline_download import register_handlers as register_od_handlers
from module.help import register_handlers as register_help_handlers
from module.torrent_search.torrent_search import register_handlers as register_torrent_handlers
from module.config_download.config_download import register_handlers as register_cf_handlers
from module.tmdb_search.tmdb_search import register_handlers as register_tmdb_handlers
from module.file_refresh.file_refresh import register_handlers as register_fl_handlers


def init_task(app: Application):
    register_search_handlers(app)
    register_storage_handlers(app)
    register_od_handlers(app)
    register_help_handlers(app)
    register_torrent_handlers(app)
    register_cf_handlers(app)
    register_tmdb_handlers(app)
    register_fl_handlers(app)
