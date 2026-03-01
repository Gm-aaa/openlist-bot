# -*- coding: UTF-8 -*-
import base64
import hashlib
import os
import shutil
import time
import urllib.parse
from pathlib import Path
from typing import Optional

import httpx
import bencode
from loguru import logger

from config.config import bot_cfg

CACHE_DIR = Path("data/torrent_cache")
CACHE_DIR.mkdir(parents=True, exist_ok=True)


class TorrentCache:
    def __init__(self, max_size: int = 10):
        self.max_size = max_size
    
    def _get_info_hash(self, torrent_data: bytes) -> str:
        try:
            data = bencode.bdecode(torrent_data)
            info = data.get(b"info", {})
            info_hash = hashlib.sha1(bencode.bencode(info)).hexdigest()
            return info_hash
        except Exception:
            return ""
    
    def _get_magnet_from_torrent(self, torrent_data: bytes) -> Optional[str]:
        try:
            data = bencode.bdecode(torrent_data)
            info = data.get("info", {})
            info_hash = hashlib.sha1(bencode.bencode(info)).hexdigest()
            
            # 转换为 Base32 编码（磁力链接标准）
            info_hash_bytes = bytes.fromhex(info_hash)
            base32_hash = base64.b32encode(info_hash_bytes).decode()
            
            # 获取 trackers
            trackers = []
            
            # 尝试从 announce-list 获取
            announce_list = data.get("announce-list", [])
            for tier in announce_list:
                for tracker in tier:
                    if isinstance(tracker, str):
                        trackers.append(tracker)
                    elif isinstance(tracker, bytes):
                        trackers.append(tracker.decode("utf-8", errors="ignore"))
            
            # 如果没有，尝试从 announce 获取
            if not trackers:
                announce = data.get("announce", "")
                if announce:
                    if isinstance(announce, str):
                        trackers.append(announce)
                    elif isinstance(announce, bytes):
                        trackers.append(announce.decode("utf-8", errors="ignore"))
            
            name = info.get("name", "")
            if isinstance(name, bytes):
                name = name.decode("utf-8", errors="ignore")
            
            magnet = f"magnet:?xt=urn:btih:{base32_hash}"
            if name:
                magnet += f"&dn={urllib.parse.quote(name)}"
            for tr in trackers[:5]:
                magnet += f"&tr={urllib.parse.quote(tr)}"
            
            return magnet
        except Exception as e:
            logger.error(f"解析种子文件失败: {e}")
            return None
    
    async def get_magnet(self, torrent_url: str) -> Optional[str]:
        cache_key = hashlib.md5(torrent_url.encode()).hexdigest()
        cache_file = CACHE_DIR / f"{cache_key}.torrent"
        magnet_file = CACHE_DIR / f"{cache_key}.magnet"
        
        if magnet_file.exists():
            with open(magnet_file, "r", encoding="utf-8") as f:
                return f.read().strip()
        
        if cache_file.exists():
            with open(cache_file, "rb") as f:
                torrent_data = f.read()
            magnet = self._get_magnet_from_torrent(torrent_data)
            if magnet:
                with open(magnet_file, "w", encoding="utf-8") as f:
                    f.write(magnet)
                return magnet
        
        try:
            async with httpx.AsyncClient() as client:
                response = await client.get(torrent_url, timeout=30)
                response.raise_for_status()
                torrent_data = response.content
            
            with open(cache_file, "wb") as f:
                f.write(torrent_data)
            
            magnet = self._get_magnet_from_torrent(torrent_data)
            if magnet:
                with open(magnet_file, "w", encoding="utf-8") as f:
                    f.write(magnet)
            
            self.cleanup_old()
            return magnet
        except Exception as e:
            logger.error(f"下载种子文件失败: {e}")
            return None
    
    def cleanup_old(self):
        files = list(CACHE_DIR.glob("*.torrent"))
        if len(files) <= self.max_size:
            return
        
        files.sort(key=lambda x: x.stat().st_mtime)
        
        for f in files[:-self.max_size]:
            try:
                cache_key = f.stem
                magnet_file = CACHE_DIR / f"{cache_key}.magnet"
                if magnet_file.exists():
                    magnet_file.unlink()
                f.unlink()
                logger.info(f"清理过期种子缓存: {f.name}")
            except Exception as e:
                logger.error(f"清理缓存失败: {e}")


torrent_cache = TorrentCache()
