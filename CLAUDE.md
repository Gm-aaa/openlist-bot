# CLAUDE.md — OpenList Bot

This file provides guidance for AI assistants working on this codebase.

---

## Project Overview

**OpenList Bot** is a Telegram bot that automates media acquisition and library management. It integrates several services into a single Telegram interface:

- **OpenList** — self-hosted file manager (open-source fork of AList)
- **PanSou** — cloud drive search aggregator (Chinese cloud storage services)
- **Prowlarr** — torrent/usenet indexer manager
- **TMDB** — movie and TV metadata
- **SmartStrm** — STRM file generation webhook
- **Jellyfin** — media server library refresh

The workflow is: search → download (offline download via OpenList) → media library refresh.

---

## Repository Structure

```
openlist-bot/
├── bot.py                         # Entry point: bot init, proxy, logging, startup
├── config.example.yaml            # Configuration template (copy to config.yaml)
├── requirements.txt               # Python dependencies
├── Dockerfile                     # Docker image definition
├── .github/workflows/
│   └── docker-image.yml           # CI/CD: builds and pushes to GHCR on master/tags
├── api/                           # External service API clients
│   ├── constants.py               # Shared constants
│   ├── openlist/
│   │   ├── openlist_api.py        # OpenList HTTP API wrapper (main client)
│   │   └── base/                  # Typed data models for OpenList responses
│   │       ├── base.py            # Generic OpenListAPIResponse[T], exceptions
│   │       ├── admin/             # MetaInfo, SettingInfo, UserInfo
│   │       ├── fs/                # FileInfo, SearchResultData, UploadTaskResult
│   │       └── storage/           # StorageInfo
│   ├── pansou/pansou_api.py       # PanSou cloud drive search client
│   ├── prowlarr/prowlarr_api.py   # Prowlarr torrent search client
│   └── tmdb/tmdb_api.py           # TMDB movie/TV metadata client
├── config/
│   └── config.py                  # YAML config loader; global bot_cfg, od_cfg
├── module/                        # Telegram command handlers (one folder per command)
│   ├── init.py                    # Registers all handlers with the Application
│   ├── help.py                    # /help, /start
│   ├── search/search.py           # /s  — PanSou cloud drive search
│   ├── torrent_search/            # /sb — Prowlarr torrent search
│   ├── tmdb_search/               # /sm — TMDB movie/TV search
│   ├── storage_browse/            # /st — Browse OpenList storage tree
│   ├── offline_download/          # /od, /ods — Submit/check offline downloads
│   ├── config_download/           # /cf — Manage bot configuration
│   └── file_refresh/              # /fl — Refresh OpenList/SmartStrm/Jellyfin
└── tools/
    ├── filters.py                 # IsAdmin / IsMember permission filters
    └── torrent_cache/             # Download, cache, and convert .torrent ↔ magnet
```

---

## Key Conventions

### Language and Style

- **Python 3.11+** — use modern type hints (`X | Y`, `list[X]`, etc.)
- **Async/await throughout** — all I/O (HTTP, Telegram callbacks) must be async
- **`httpx.AsyncClient`** — the standard async HTTP client; never use `requests`
- **`loguru`** — the only logger; use `from loguru import logger`; never use `logging`
- **`PyYAML`** — configuration is YAML; never use `.env` or `configparser`
- No test suite currently exists; do not add test boilerplate unless asked

### Configuration

- Config is loaded from `config.yaml` (git-ignored) at startup via `config/config.py`
- Use `config.example.yaml` as the canonical template for all new config keys
- Global singletons exposed from `config/config.py`:
  - `bot_cfg` — `BotConfig` instance (all settings)
  - `od_cfg` — `OfflineDownload` subset for download defaults
- When adding a new config key, add it to `config.example.yaml` with a placeholder value and a comment

### Module Structure Pattern

Each command module follows this layout:

```
module/<feature>/
└── <feature>.py        # Contains handler functions + a Page/State class if needed
```

All handlers are registered in `module/init.py`. When adding a new command:
1. Create `module/<feature>/<feature>.py`
2. Import and register its handlers in `module/init.py`
3. Add the command to the help text in `module/help.py`

### Pagination Pattern

Commands that display lists (search results, storage trees) use a local `Page` class:

```python
PAGE: dict[str, Page] = {}   # keyed by f"{chat_id}|{message_id}"

class Page:
    def __init__(self, results, ...): ...
    def render(self) -> tuple[str, InlineKeyboardMarkup]: ...
    def next(self): ...
    def prev(self): ...
```

The cache is stored in a module-level dict; there is no shared cache mechanism.

### Multi-Step Conversation State

Commands with multiple steps (e.g., `/od`, `/cf`) use integer constants for states:

```python
STEP_SELECTING_TOOL = 0
STEP_BROWSING_PATH  = 1
STEP_ENTERING_URL   = 2
STEP_CONFIRMING     = 3
```

State data is stored in the module-level `chat_data` dict from `config/config.py`.

### Path ID System

Callback data in Telegram inline keyboards has a 64-byte limit. Long file paths are stored in a short-ID registry (a dict mapping integers to path strings) and the integer key is used in callback data. This pattern exists in `torrent_search.py` and `offline_download.py`.

### Permission Filters

```python
from tools.filters import IsAdmin, IsMember
```

- `IsAdmin` — restricts to the single admin UID in `bot_cfg.admin`
- `IsMember` — allows admin + any UID listed in `bot_cfg.member`
- Apply filters on handler registration: `filters=IsAdmin()`

### API Client Pattern

All API clients are thin async wrappers around `httpx.AsyncClient`:

```python
class SomeAPI:
    def __init__(self, host: str, token: str):
        self._client = httpx.AsyncClient(base_url=host, timeout=10)

    async def some_method(self) -> SomeResult:
        r = await self._client.get("/endpoint", headers={"Authorization": self.token})
        r.raise_for_status()
        return SomeResult(**r.json())
```

For OpenList responses, use the generic `OpenListAPIResponse[T]` wrapper defined in `api/openlist/base/base.py`.

---

## Commands Reference

| Command | Access | Description |
|---------|--------|-------------|
| `/start` | All | Welcome message |
| `/help` | Admin | Full command reference |
| `/s <query>` | All | Search cloud drives via PanSou |
| `/sb <query>` | All | Search torrents via Prowlarr |
| `/sm <query>` | All | Search movies/TV via TMDB |
| `/st` | Admin | Browse OpenList storage tree |
| `/od` | Admin | Submit an offline download task |
| `/ods` | Admin | Check offline download task status |
| `/cf` | Admin | Configure default download path/tool |
| `/fl` | Admin | Refresh OpenList / SmartStrm / Jellyfin |
| `/menu` | Admin | Refresh bot command menu with Telegram |
| `/px` | Admin | Toggle proxy on/off at runtime |

---

## Data Flow: Offline Download

```
/od  →  select tool  →  browse path  →  enter URL  →  confirm
              ↓                               ↓
         od_cfg default             OpenList add_offline_download()
```

After submission, `/ods` polls `get_offline_download_undone_task()` and `get_offline_download_done_task()`.

---

## Global Singletons

| Variable | Type | Location | Purpose |
|----------|------|----------|---------|
| `bot_cfg` | `BotConfig` | `config/config.py` | All config values |
| `od_cfg` | `OfflineDownload` | `config/config.py` | Download defaults |
| `chat_data` | `dict` | `config/config.py` | Per-chat conversation state |
| `openlist` | `OpenListAPI` | `bot.py` | OpenList HTTP client |
| `pansou` | `PanSouAPI` | `bot.py` | PanSou HTTP client |
| `torrent_cache` | `TorrentCache` | `bot.py` | .torrent file cache |

TMDB uses an internal singleton via `_tmdb_api` inside `api/tmdb/tmdb_api.py`.

---

## Logging

```python
from loguru import logger

logger.info("message")
logger.error("message")
```

- Log files rotate at 5 MB, stored in `logs/bot.log`
- Level is controlled by `log_level` in `config.yaml` (default: `INFO`)
- Noisy libraries (httpx, telegram, apscheduler) are silenced to WARNING

---

## Docker and Deployment

### Build

```bash
docker build -t openlist-bot .
```

### Run (recommended)

```bash
docker run -d --name openlist-bot \
  -v $(pwd)/config.yaml:/app/config.yaml \
  -v $(pwd)/logs:/app/logs \
  --restart always \
  ghcr.io/gm-aaa/openlist-bot:latest
```

### Manual

```bash
pip install -r requirements.txt
cp config.example.yaml config.yaml
# Fill in config.yaml
python bot.py
```

### CI/CD

GitHub Actions (`.github/workflows/docker-image.yml`) automatically builds and pushes to `ghcr.io/gm-aaa/openlist-bot` on:
- Push to `master` → tagged `:latest`
- Tag push `v*` → tagged with the version number

---

## Adding a New Integration

1. Create `api/<service>/<service>_api.py` with an async client class
2. Define typed dataclasses for response models
3. Instantiate the client in `bot.py` (post-init or at module level)
4. Create `module/<feature>/<feature>.py` with handler functions
5. Register handlers in `module/init.py`
6. Add the command to help text in `module/help.py`
7. Add any required config keys to `config.example.yaml`

---

## Known Constraints

- `config.yaml` is never committed — always use `config.example.yaml` as the reference
- The torrent cache is file-based (`data/torrent_cache/`); its max size is controlled by `prowlarr.torrent_cache_max` in config
- Telegram callback data is limited to 64 bytes; use the short-ID path registry for long strings
- No database — all runtime state is in-memory and lost on restart (except persisted config changes written back to `config.yaml`)
- The admin is a single integer UID; there is no role hierarchy beyond admin/member/public
