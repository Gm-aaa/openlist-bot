# -*- coding: UTF-8 -*-
from dataclasses import dataclass
from typing import Optional
import httpx
from loguru import logger

from api.constants import USER_AGENT


@dataclass
class TMDbResult:
    id: int
    title: str
    original_title: str
    overview: str
    release_date: str
    poster_path: Optional[str]
    media_type: str
    vote_average: float


class TMDbAPI:
    def __init__(self, api_key: str):
        self.api_key = api_key
        self.base_url = "https://api.themoviedb.org/3"
        self.image_base = "https://image.tmdb.org/t/p/w200"
        self.client = httpx.AsyncClient(headers={"User-Agent": USER_AGENT}, timeout=10)
    
    async def search(self, query: str, language: str = "zh-CN") -> list[TMDbResult]:
        """搜索电影/电视剧"""
        if not self.api_key:
            logger.error("TMDB API Key 未配置")
            return []
        
        url = f"{self.base_url}/search/multi"
        params = {
            "api_key": self.api_key,
            "query": query,
            "language": language,
            "include_adult": False
        }
        
        try:
            response = await self.client.get(url, params=params)
            data = response.json()
            
            results = []
            if data.get("results"):
                for item in data["results"][:10]:
                    if item.get("media_type") not in ("movie", "tv"):
                        continue
                    
                    media_type = "movie" if item.get("media_type") == "movie" else "tv"
                    title = item.get("title") or item.get("name") or ""
                    original_title = item.get("original_title") or item.get("original_name") or title
                    release_date = item.get("release_date") or item.get("first_air_date") or ""
                    
                    overview = item.get("overview", "")
                    if len(overview) > 200:
                        overview = overview[:200] + "..."
                    
                    results.append(TMDbResult(
                        id=item.get("id", 0),
                        title=title,
                        original_title=original_title,
                        overview=overview if overview else "暂无简介",
                        release_date=release_date[:4] if release_date else "未知",
                        poster_path=item.get("poster_path"),
                        media_type=media_type,
                        vote_average=item.get("vote_average", 0) or 0
                    ))
            
            return results
        except Exception as e:
            logger.error(f"TMDB 搜索失败: {e}")
            return []


_tmdb_api: Optional[TMDbAPI] = None


def get_tmdb(api_key: str = "") -> TMDbAPI:
    global _tmdb_api
    if not _tmdb_api or (_tmdb_api.api_key != api_key):
        _tmdb_api = TMDbAPI(api_key)
    return _tmdb_api
