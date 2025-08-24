# Swiss Knife 🔧

一套用 Rust 编写的实用命令行工具集合，专注于性能和用户体验。

[中文版](README-CN.md) | [English](README.md)

## 概述

Swiss Knife 是一个不断增长的命令行工具集合，使用 Rust 编写，旨在高效处理常见任务，并提供美观的用户界面。

## 可用工具

### 1. convert - 视频转录和内容生成

高性能的视频转录工具，支持 AI 驱动的内容生成。

**功能特性：**

- 🚀 **并发处理**：利用 Tokio 异步运行时实现并行分块转录
- 📊 **实时进度**：为每个处理步骤提供优雅的进度条
- ♻️ **智能缓存**：自动检测并复用已处理的文件
- 🎯 **自动分块**：长视频自动分割并并行处理
- 🎨 **彩色输出**：用户友好的彩色终端输出
- 📦 **自动压缩**：大音频文件自动压缩以满足 API 限制

## 安装

### 从 crates.io 安装（推荐）

```bash
cargo install swiss-knife
```

### 从源码安装

```bash
# 克隆仓库
git clone https://github.com/tyrchen/swiss-knife
cd swiss-knife

# 发布模式构建
cargo build --release

# 安装到 cargo bin 目录
cargo install --path .
```

## 使用方法

### convert - 视频转录

```bash
# 设置 OpenAI API 密钥
export OPENAI_API_KEY="your-api-key"

# 处理视频文件
convert <video_file>

# 示例
convert ~/Videos/lecture.mp4
```

**输出示例：**

```text
🎬 Processing video: "lecture.mp4"

⠋ Analyzing video duration...
Video duration: 2500 seconds

⚠️  Video longer than 1300 seconds, processing in chunks...
   Will create 2 chunks

⠏ [########################################] 2/2 chunks processed
✅ All chunks merged into complete transcript

⠋ Generating content with GPT-5-mini...
✅ Content generated successfully!

✨ Processing complete!
📦 All files saved in /tmp
```

**生成的文件：**

- 📝 转录文本：完整的视频转录内容
- 📋 完整内容：JSON 格式的所有生成内容
- 🏷️ 标题列表：3 个备选标题
- 📄 视频描述：2 段详细描述
- 💬 动态文案：3 个 Bilibili 动态文案

### 更多工具即将推出

敬请期待 Swiss Knife 工具集合中添加更多实用工具！

## 系统要求

### convert 工具需要

- FFmpeg（用于音频提取）
- FFprobe（用于视频分析）
- OpenAI API 密钥

### 通用要求

- Rust 1.70+
- Linux/macOS/Windows

## 贡献

欢迎贡献！您可以：

- 向集合中添加新工具
- 改进现有工具
- 报告问题或建议功能
- 提交拉取请求

## 路线图

- [ ] 添加更多视频/音频处理工具
- [ ] 添加文本处理工具
- [ ] 添加文件管理工具
- [ ] 添加网络工具
- [ ] 添加开发辅助工具

## 许可证

MIT 许可证 - 详见 [LICENSE](LICENSE) 文件。

## 作者

Tyr Chen <tyr.chen@gmail.com>
