# ============ 构建阶段 ============
FROM rust:latest AS builder

WORKDIR /app

# 安装必要的系统库（编译 openssl/native-tls 需要）
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# 复制源码并编译
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

# ============ 运行阶段 ============
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# 从构建阶段复制编译好的二进制文件
COPY --from=builder /app/target/release/openlist-bot .

# 创建日志和数据目录
RUN mkdir -p logs data/torrent_cache data/downloads

# 启动命令
CMD ["./openlist-bot"]
