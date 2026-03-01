# -*- coding: UTF-8 -*-
from dataclasses import dataclass
from typing import List, Optional
from urllib.parse import urljoin

import httpx
from loguru import logger

from api.constants import USER_AGENT
from config.config import bot_cfg


@dataclass
class ProwlarrResult:
    title: str
    size: int
    seeders: int
    leechers: int
    magnet_url: str
    torrent_url: str
    indexer: str
    categories: List[str]
    
    @property
    def size_str(self) -> str:
        if self.size < 1024:
            return f"{self.size} B"
        elif self.size < 1024 * 1024:
            return f"{self.size / 1024:.1f} KB"
        elif self.size < 1024 * 1024 * 1024:
            return f"{self.size / (1024 * 1024):.1f} MB"
        else:
            return f"{self.size / (1024 * 1024 * 1024):.2f} GB"


class ProwlarrAPI:
    def __init__(self, host: str, api_key: str):
        self.host = host
        self.api_key = api_key
        self.headers = {
            "User-Agent": USER_AGENT,
            "Content-Type": "application/json",
            "X-Api-Key": api_key,
        }

    async def search(
        self,
        query: str,
        categories: Optional[List[int]] = None,
        indexer_ids: Optional[List[int]] = None,
        limit: int = 20
    ) -> List[ProwlarrResult]:
        if not self.host or not self.api_key:
            raise Exception("Prowlarr 未配置，请在 config.yaml 中配置 prowlarr_host 和 prowlarr_api_key")
        
        url = urljoin(self.host, "/api/v1/search")
        
        params = {
            "query": query,
            "limit": limit,
        }
        
        if categories:
            params["categories"] = categories
        if indexer_ids:
            params["indexerIds"] = indexer_ids
        
        logger.info(f"Prowlarr 搜索: {query}")
        
        async with httpx.AsyncClient() as client:
            response = await client.get(url, params=params, headers=self.headers, timeout=30)
        
        logger.info(f"Prowlarr 搜索响应: {response.status_code}")
        
        if response.status_code == 401:
            raise Exception("Prowlarr API Key 错误")
        
        response.raise_for_status()
        results = response.json()
        
        logger.info(f"Prowlarr 搜索结果: {len(results)} 条")
        
        prowlarr_results = []
        for item in results:
            magnet_url = item.get("magnetUrl", "")
            torrent_url = item.get("downloadUrl", "")
            
            if not magnet_url and not torrent_url:
                continue
            
            categories_list = item.get("categories", [])
            if isinstance(categories_list, list):
                categories_str = [str(c) for c in categories_list]
            else:
                categories_str = []
            
            prowlarr_results.append(ProwlarrResult(
                title=item.get("title", ""),
                size=item.get("size", 0),
                seeders=item.get("seeders", 0),
                leechers=item.get("leechers", 0),
                magnet_url=magnet_url,
                torrent_url=torrent_url,
                indexer=item.get("indexer", ""),
                categories=categories_str,
            ))
        
        return prowlarr_results
    
    async def get_indexers(self) -> List[dict]:
        if not self.host or not self.api_key:
            raise Exception("Prowlarr 未配置")
        
        url = urljoin(self.host, "/api/v1/indexer")
        
        async with httpx.AsyncClient() as client:
            response = await client.get(url, headers=self.headers, timeout=30)
        
        response.raise_for_status()
        return response.json()
    
    async def get_categories(self) -> List[dict]:
        if not self.host or not self.api_key:
            raise Exception("Prowlarr 未配置")
        
        url = urljoin(self.host, "/api/v1/indexer/categories")
        
        async with httpx.AsyncClient() as client:
            response = await client.get(url, headers=self.headers, timeout=30)
        
        response.raise_for_status()
        return response.json()


def get_prowlarr() -> ProwlarrAPI:
    return ProwlarrAPI(
        bot_cfg.prowlarr_host,
        bot_cfg.prowlarr_api_key
    )
