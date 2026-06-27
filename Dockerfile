# ============ 构建阶段 ============
FROM rust:1.83-bookworm AS builder

WORKDIR /app

# 复制依赖清单以利用 Docker 缓存
COPY Cargo.toml Cargo.lock ./

# 创建空的 src/main.rs 以构建依赖层
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src

# 复制源码并构建
COPY src ./src
RUN touch src/main.rs && cargo build --release

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
