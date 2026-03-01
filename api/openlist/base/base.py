from dataclasses import dataclass
from typing import TypeVar, Generic, Type, Union
from loguru import logger

T = TypeVar("T")


class OpenListAPIData:
    @classmethod
    def from_dict(cls, data: dict) -> Union["OpenListAPIData", list["OpenListAPIData"]]:
        raise NotImplementedError

    def to_dict(self) -> dict:
        return vars(self)


@dataclass
class OpenListAPIResponse(Generic[T]):
    def __init__(self, code: int, message: str, data: T | list[T] | dict | None = None):
        self.code = code
        self.message = message
        self.data = data
        self.raw_data: dict | list | None = None

    @classmethod
    def from_dict(
        cls, result: dict, data_class: Type[T] | None
    ) -> "OpenListAPIResponse[T]":
        response = cls(code=result.get("code"), message=result.get("message"))
        response._check_code()
        data = result.get("data")
        response.raw_data = data
        
        if data_class and data:
            try:
                response.data = data_class.from_dict(data)
            except Exception as e:
                logger.error(f"from_dict 转换失败: {e}")
                response.data = data
        else:
            response.data = data
            
        return response

    def _check_code(self):
        if self.code == 401 and self.message == "that's not even a token":
            raise OpenListTokenError()

    def __repr__(self):
        return (
            f"APIResponse(code={self.code}, message={self.message}, data={self.data})"
        )


class OpenListError(Exception):
    def __init__(self, message: str):
        self.message = message

    def __str__(self):
        return self.message


class OpenListTokenError(OpenListError):
    def __init__(self):
        super().__init__("OpenList Token错误")
