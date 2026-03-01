# -*- coding: UTF-8 -*-
import base64
import hashlib
import hmac
import os
import time
from typing import Literal, Type, Dict, Any
from urllib import parse
from urllib.parse import urljoin

import httpx
from loguru import logger

from api.constants import USER_AGENT
from api.openlist.base import *
from api.openlist.base.base import OpenListAPIResponse, T
from config.config import bot_cfg


class OpenListAPI:
    def __init__(self, host, token=None):
        self.host = host
        self._token = token
        self._jwt_token = None
        self._token_expires_at = 0

        self.headers = {
            "User-Agent": USER_AGENT,
            "Content-Type": "application/json",
        }
        self._update_auth_header()

    def _update_auth_header(self):
        if self._jwt_token:
            self.headers["Authorization"] = self._jwt_token
        elif self._token:
            self.headers["Authorization"] = self._token

    async def login(self):
        if self._token:
            self._update_auth_header()
            return True
        return False

    async def _ensure_token(self):
        if self._jwt_token and time.time() < self._token_expires_at - 60:
            return True
        if self._token:
            self._update_auth_header()
            return True
        return False

    def _get_auth_token(self) -> str:
        return self._jwt_token or self._token or ""

    async def _request(
        self,
        method: Literal["GET", "POST", "PUT"],
        url,
        *,
        data_class: Type[T] = None,
        headers: Dict[str, str] = None,
        json: Dict[str, Any] = None,
        params: Dict[str, Any] = None,
        data: Any = None,
        timeout: int = 10,
    ) -> OpenListAPIResponse[T]:
        url = urljoin(self.host, url)
        req_headers = {**self.headers, **(headers or {})}
        req_headers["Authorization"] = self._get_auth_token()

        async with httpx.AsyncClient() as client:
            if method == "GET":
                response = await client.get(
                    url, headers=req_headers, params=params, timeout=timeout
                )
            elif method == "POST":
                response = await client.post(
                    url, headers=req_headers, json=json, timeout=timeout
                )
            elif method == "PUT":
                response = await client.put(
                    url, headers=req_headers, data=data, timeout=timeout
                )
        
        response.raise_for_status()
        result = response.json()
        
        return OpenListAPIResponse.from_dict(result, data_class)

    async def search(
        self,
        keywords,
        page: int = 1,
        per_page: int = 100,
        parent: str = "/",
        scope: int = 0,
        password: str = "",
    ):
        """搜索文件"""
        await self._ensure_token()
        body = {
            "parent": parent,
            "keywords": keywords,
            "scope": scope,
            "page": page,
            "per_page": per_page,
            "password": password,
        }
        return await self._request(
            "POST", "/api/fs/search", data_class=SearchResultData, json=body
        )

    async def fs_get(self, path):
        """获取下载信息"""
        await self._ensure_token()
        return await self._request(
            "POST", "/api/fs/get", data_class=FileInfo, json={"path": path}
        )

    async def storage_get(self, storage_id):
        """查询指定存储信息"""
        await self._ensure_token()
        url = f"/api/admin/storage/get?id={storage_id}"
        return await self._request("GET", url, data_class=StorageInfo)

    async def storage_create(self, body: StorageInfo | dict):
        """新建存储"""
        await self._ensure_token()
        url = "/api/admin/storage/create"
        if isinstance(body, dict):
            body = StorageInfo.from_dict(body)
        return await self._request("POST", url, json=body.to_dict())

    async def storage_update(self, body: StorageInfo):
        """更新存储"""
        await self._ensure_token()
        url = "/api/admin/storage/update"
        if isinstance(body, dict):
            body = StorageInfo.from_dict(body)
        return await self._request("POST", url, json=body.to_dict())

    async def storage_list(self):
        """获取存储列表"""
        await self._ensure_token()
        url = "/api/admin/storage/list"
        return await self._request("GET", url, data_class=StorageInfo)

    async def storage_delete(self, storage_id) -> OpenListAPIResponse:
        """删除指定存储"""
        await self._ensure_token()
        url = f"/api/admin/storage/delete?id={str(storage_id)}"
        return await self._request("POST", url)

    async def storage_enable(self, storage_id) -> OpenListAPIResponse:
        """开启存储"""
        await self._ensure_token()
        url = f"/api/admin/storage/enable?id={str(storage_id)}"
        return await self._request("POST", url)

    async def storage_disable(self, storage_id) -> OpenListAPIResponse:
        """关闭存储"""
        await self._ensure_token()
        url = f"/api/admin/storage/disable?id={str(storage_id)}"
        return await self._request("POST", url)

    async def upload(
        self,
        local_path,
        remote_path,
        file_name,
        as_task: Literal["true", "false"] = "false",
    ):
        """上传文件"""
        await self._ensure_token()
        url = "/api/fs/put"
        header = {
            "UserAgent": USER_AGENT,
            "As-Task": as_task,
            "Authorization": self._get_auth_token(),
            "File-Path": parse.quote(f"{remote_path}/{file_name}"),
            "Content-Length": f"{os.path.getsize(local_path)}",
        }
        with open(local_path, "rb") as f:
            file_data = f.read()
        return await self._request(
            "PUT",
            url,
            headers=header,
            data=file_data,
        )

    async def fs_list(self, path, per_page: int = 0):
        """获取列表，强制刷新列表"""
        await self._ensure_token()
        url = "/api/fs/list"
        body = {"path": path, "page": 1, "per_page": per_page, "refresh": True}
        return await self._request("POST", url, json=body)

    async def driver_list(self):
        """获取驱动列表"""
        await self._ensure_token()
        url = "/api/admin/driver/list"
        return await self._request("GET", url)

    async def setting_list(self):
        """获取设置列表"""
        await self._ensure_token()
        url = "/api/admin/setting/list"
        return await self._request("GET", url, data_class=SettingInfo)

    async def user_list(self):
        """获取用户列表"""
        await self._ensure_token()
        url = "/api/admin/user/list"
        return await self._request("GET", url, data_class=UserInfo)

    async def meta_list(self):
        """获取元信息列表"""
        await self._ensure_token()
        url = "/api/admin/meta/list"
        return await self._request("GET", url, data_class=MetaInfo)

    async def setting_get(self, key):
        """获取某项设置"""
        await self._ensure_token()
        url = "/api/admin/setting/get"
        params = {"key": key}
        return await self._request("GET", url, data_class=SettingInfo, params=params)

    async def get_offline_download_tools(self):
        """获取离线下载工具"""
        await self._ensure_token()
        url = "/api/public/offline_download_tools"
        return await self._request("GET", url)

    async def add_offline_download(self, urls, tool, path, delete_policy):
        """离线下载"""
        await self._ensure_token()
        url = "/api/fs/add_offline_download"
        body = {
            "delete_policy": str(delete_policy),
            "path": path,
            "tool": tool,
            "urls": urls,
        }
        return await self._request("POST", url, json=body)

    async def get_offline_download_undone_task(self):
        """获取离线下载未完成任务"""
        await self._ensure_token()
        url = "/api/admin/task/offline_download/undone"
        return await self._request("GET", url)

    async def get_offline_download_done_task(self):
        """获取离线下载已完成任务"""
        await self._ensure_token()
        url = "/api/admin/task/offline_download/done"
        return await self._request("GET", url)

    async def clear_offline_download_done_task(self):
        """清空离线下载已完成任务（包含成功/失败）"""
        await self._ensure_token()
        url = "/api/admin/task/offline_download/clear_done"
        return await self._request("POST", url)

    @staticmethod
    def sign(path) -> str:
        """计算签名"""
        expire_time_stamp = "0"
        to_sign = f"{path}:{expire_time_stamp}"
        signature = hmac.new(
            bot_cfg.openlist_token.encode(), to_sign.encode(), hashlib.sha256
        ).digest()
        _safe_base64 = base64.urlsafe_b64encode(signature).decode()
        return f"{_safe_base64}:{expire_time_stamp}"


openlist = OpenListAPI(
    bot_cfg.openlist_host, 
    bot_cfg.openlist_token
)
