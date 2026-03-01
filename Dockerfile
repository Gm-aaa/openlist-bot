FROM python:3.11-slim

# 设置工作目录
WORKDIR /app

# 设置环境变量
ENV PYTHONDONTWRITEBYTECODE 1
ENV PYTHONUNBUFFERED 1

# 安装系统依赖（如需 SOCKS 代理支持）
RUN apt-get update && apt-get install -y --no-install-recommends     gcc     python3-dev     && rm -rf /var/lib/apt/lists/*

# 先复制依赖文件，利用 Docker 缓存
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# 复制项目代码
COPY . .

# 创建日志和数据目录
RUN mkdir -p logs data/torrent_cache data/downloads

# 启动命令
CMD ["python", "bot.py"]
