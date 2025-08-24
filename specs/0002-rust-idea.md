# Rust 视频转录和内容生成工具

## 功能描述

使用 Rust 实现的高性能视频转录和内容生成工具，采用异步并发架构处理音频提取、转录和内容生成。

## 架构设计

基于 Tokio 异步运行时，使用 MPSC channel 协调并行任务：

1. **并行音频提取**：使用 Rayon 并行调用 ffmpeg 分块提取音频
2. **并发转录**：通过 Tokio 任务并发调用 OpenAI API 转录
3. **流式处理**：MPSC channel 连接提取和转录任务，实现流水线处理

## 核心模块

### openai.rs - OpenAI API 通用封装

```rust
// OpenAI API 客户端封装
pub struct OpenAIClient {
    client: reqwest::Client,
    api_key: String,
}

// 音频转录接口
pub async fn transcribe(audio_data: Vec<u8>) -> Result<String>

// 内容生成接口
pub async fn generate_content(prompt: String) -> Result<ContentResponse>
```

### convert.rs - CLI 入口和主逻辑

```rust
// 主要功能：
// 1. 解析命令行参数
// 2. 调用 ffmpeg 提取和分块音频
// 3. 创建 Tokio 任务池并发转录
// 4. 合并转录结果并生成内容
// 5. 保存所有输出文件
```

## 核心功能实现

### 1. 音频提取与分块

- 使用 `std::process::Command` 调用 ffmpeg
- 自动检测视频时长，超过 1300 秒自动分块
- 输出格式：MP3 32kbps, 16kHz, 单声道
- 使用 Rayon 并行处理多个分块

### 2. 并发转录

- 使用 `tokio::spawn` 创建并发任务
- 每个音频块独立转录，通过 MPSC channel 收集结果
- 自动重试失败的请求（最多 3 次）
- 实现缓存机制，跳过已存在的转录文件

### 3. 内容生成

- 合并所有转录文本
- 构建 prompt 调用 GPT-5-mini API
- 解析 JSON 响应提取：
  - 3 个标题（≤16 字）
  - 2 段描述（300-500 字/段）
  - 3 个动态文案（150-250 字/个）

### 4. 错误处理

- 使用 `anyhow::Result` 统一错误处理
- 详细的错误上下文信息
- 优雅的失败恢复机制

## 技术栈

- **异步运行时**: `tokio` - 高性能异步 I/O
- **HTTP 客户端**: `reqwest` with `rustls-tls` - 安全的 HTTPS 请求
- **错误处理**: `anyhow` - 简洁的错误处理
- **并行处理**: `rayon` - CPU 密集型任务并行化
- **序列化**: `serde_json` - JSON 解析和生成

## 性能优化

1. **并发控制**：使用 Semaphore 限制并发任务数（默认 5）
2. **内存优化**：流式读取大文件，避免一次性加载
3. **连接复用**：共享 reqwest Client 实例
4. **智能重试**：指数退避算法处理 API 限流

## 使用方法

```bash
# 编译
cargo build --release

# 运行
~/.target/release/convert <video_file>

# 环境变量
export OPENAI_API_KEY="your-api-key"
export OPENAI_BASE_URL="https://api.openai.com/v1"  # 可选
```

## 输出文件

保持与原版相同的输出结构（`/tmp` 目录）：

- 音频文件：`.mp3` 格式
- 转录文本：`.txt` 格式
- 生成内容：`.json` 和分离的 `.txt` 文件

## 依赖配置

```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "multipart", "stream"] }
anyhow = "1.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
clap = { version = "4", features = ["derive"] }
rayon = "1.8"
```

## 错误码

- `1`: 命令行参数错误
- `2`: 文件访问错误
- `3`: ffmpeg 执行失败
- `4`: API 调用失败
- `5`: JSON 解析错误
