# -*- coding: UTF-8 -*-
from dataclasses import dataclass
from pathlib import Path
from typing import Optional, Any
import sys
import os

import yaml

# 存储和检索与特定聊天相关联的数据
chat_data = {}
DOWNLOADS_PATH = Path("data/downloads")
DOWNLOADS_PATH.mkdir(parents=True, exist_ok=True)

# 统一配置文件路径
CONFIG_FILE = "config.yaml"


class BaseConfig:
    def __init__(self, cfg_path):
        self.cfg_path = cfg_path
        self.config = self.load_config()
        self._key_map = {}

    def load_config(self):
        if not os.path.exists(self.cfg_path):
            print(f"❌ 错误: 找不到配置文件 '{self.cfg_path}'")
            print(f"💡 请参考项目中的 'config.example.yaml' 创建该文件。")
            sys.exit(1)
            
        with open(self.cfg_path, "r", encoding="utf-8") as f:
            try:
                cfg = yaml.safe_load(f)
                if not cfg:
                    return {}
                return cfg
            except Exception as e:
                print(f"❌ 错误: 配置文件格式不正确: {e}")
                sys.exit(1)

    def save_config(self):
        with open(self.cfg_path, "w", encoding="utf-8") as f:
            yaml.dump(self.config, f, allow_unicode=True)

    def retrieve(self, key: str, default: Optional[Any] = None) -> Any:
        keys = key.split(".")
        result = self.config
        for k in keys:
            if isinstance(result, dict):
                result = result.get(k, default)
            else:
                return default
        self._key_map[keys[-1]] = key
        return result

    def modify(self, key, value):
        keys = key.split(".")
        temp = self.config
        for k in keys[:-1]:
            temp = temp.setdefault(k, {})
        temp[keys[-1]] = value
        self.save_config()


class Config(BaseConfig):
    def __setattr__(self, key, value):
        if key in self.__dict__ and key in self._key_map:
            self.modify(self._key_map[key], value)
        super().__setattr__(key, value)


class BotConfig(Config):
    def __init__(self, cfg_path):
        super().__init__(cfg_path)
        self.admin: int = self.retrieve("user.admin")
        self.member: list = self.retrieve("user.member", [])
        self.bot_token = self.retrieve("user.bot_token")

        self.openlist_host = self.retrieve("openlist.openlist_host")
        self.openlist_web = self.retrieve("openlist.openlist_web")
        self.openlist_token = self.retrieve("openlist.openlist_token")

        self.pansou_host = self.retrieve("pansou.pansou_host")
        self.pansou_token = self.retrieve("pansou.pansou_token", "")

        self.prowlarr_host = self.retrieve("prowlarr.prowlarr_host")
        self.prowlarr_api_key = self.retrieve("prowlarr.prowlarr_api_key")
        self.torrent_cache_max = self.retrieve("prowlarr.torrent_cache_max", 10)
        
        self.tmdb_api_key = self.retrieve("tmdb.tmdb_api_key", "")
        
        self.smartstrm_url = self.retrieve("smartstrm.smartstrm_url", "")
        self.task_name = self.retrieve("smartstrm.task_name", "")
        
        self.jellyfin_host = self.retrieve("jellyfin.jellyfin_host", "")
        self.jellyfin_api_key = self.retrieve("jellyfin.jellyfin_api_key", "")
        
        self.proxy_enable = self.retrieve("proxy.enable", False)
        self.hostname = self.retrieve("proxy.hostname")
        self.port = self.retrieve("proxy.port")
        self.scheme = self.retrieve("proxy.scheme")

        self.log_level = self.retrieve("log_level", "INFO")


class OfflineDownload(Config):
    def __init__(self, cfg_path):
        super().__init__(cfg_path)
        self.download_tool = self.retrieve("openlist.download_tool")
        self.download_path = self.retrieve("openlist.download_path")
        self.download_strategy = self.retrieve("openlist.download_strategy")
        self.download_url = self.retrieve("openlist.download_url")


def reload_od_cfg():
    """重新加载离线下载配置"""
    global od_cfg
    od_cfg = OfflineDownload(CONFIG_FILE)
    return od_cfg


bot_cfg = BotConfig(CONFIG_FILE)
od_cfg = OfflineDownload(CONFIG_FILE)
