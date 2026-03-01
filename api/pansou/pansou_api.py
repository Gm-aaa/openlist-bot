# -*- coding: UTF-8 -*-
from dataclasses import dataclass
from urllib.parse import urljoin

import httpx
from loguru import logger

from api.constants import USER_AGENT
from config.config import bot_cfg


@dataclass
class PanSouResult:
    name: str
    url: str
    size: str
    source: str
    pan_type: str
    password: str = ""


class PanSouAPI:
    def __init__(self, host, token=None):
        self.host = host
        self._token = token
        self.headers = {
            "User-Agent": USER_AGENT,
            "Content-Type": "application/json",
        }
        if self._token:
            self.headers["Authorization"] = f"Bearer {self._token}"

    async def search(self, keyword: str, page: int = 1) -> list[PanSouResult]:
        url = urljoin(self.host, "/api/search")
        body = {
            "kw": keyword,
            "res": "merge",
            "src": "all"
        }
        
        logger.info(f"PanSou 搜索: {keyword}, page: {page}")
        
        async with httpx.AsyncClient() as client:
            response = await client.post(url, json=body, headers=self.headers, timeout=30)
        
        logger.info(f"PanSou 搜索响应: {response.status_code}")
        
        response.raise_for_status()
        result = response.json()
        
        logger.info(f"PanSou 搜索结果: code={result.get('code')}")
        
        if result.get("code") != 0 and result.get("code") != 200:
            raise Exception(result.get("message", "Search failed"))
        
        data = result.get("data", {})
        merged = data.get("merged_by_type", {}) if isinstance(data, dict) else {}
        
        results = []
        for pan_type, links in merged.items():
            if isinstance(links, list):
                for link in links:
                    name = link.get("note", "").replace("\xa0", " ").strip() if link.get("note") else ""
                    results.append(PanSouResult(
                        name=name,
                        url=link.get("url", ""),
                        size=link.get("size", ""),
                        source=link.get("source", ""),
                        pan_type=pan_type,
                        password=link.get("password", "")
                    ))
        
        return results


pansou = PanSouAPI(
    bot_cfg.pansou_host,
    bot_cfg.pansou_token
)
