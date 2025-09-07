# Imgen

Imgen 是一个 CLI 工具，用于生成图片。它将读取一个 YAML 文件，该文件包含主题、提示和图像生成指令。然后，它将使用 OpenAI 的 API 生成图像，并将图像保存到指定的目录中。

yaml 文件格式：

```yaml
system_prompt: |
  You are a helpful assistant that can generate images based on the given prompt.
  You will be given a prompt and you will need to generate an image based on the prompt.
  The image should be in the style of the given theme.
style: landscape # landscale or square / portrait
themes:
  - name: ghibli
    instructions: please generate ghibli style image
  - name: futuristic
    instructions: please generate futuristic style image
prompts:
  - name: memory-safety
    prompt: |
      technical illustration explaining Rust memory safety:
        - Peaceful magical lab with animated arrows showing data flow between stack and heap
        - Characters representing Ownership, Borrow (&), Mutable Borrow (&mut), and Lifetime ('a)
        - A friendly compile-time checker character preventing dangling pointers and use-after-free
        - Warm colors, soft glowing labels: "Stack", "Heap", "Ownership", "&", "&mut", "'a"
        - Wide composition, lush background but with clear technical annotations
  - name: concurrency-safety
    prompt: |
      diagram illustrating Rust's concurrency safety:
      - Two cozy threads depicted as lively creatures exchanging data via an Arc<Mutex<T>> chest
      - Magical lock representing Mutex, glowing channel pipes for Send/Sync
      - A guardian spirit showing "no data races" shield at compile time
      - Labels: "Thread 1", "Thread 2", "Arc<Mutex<T>>", "Send", "Sync"
      - Wide, bright, friendly, but technically accurate
```

CLI 需要根据 theme name 创建对应的目录，并保存生成的图片。每个图片的文件名是 prompt name slug + <first 6 chars of prompt hash>.png。格式为 png。

prompt hash: `blake3(system_prompt + theme_instruction + prompt)`

这样的命名方式可以在 prompt 不变的情况下避免重复生成图片。

请使用 openai gpt-image-1 模型。它是最新的图片生成模型。使用方式：

```bash
curl https://api.openai.com/v1/images/generations \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -d '{
    "model": "gpt-image-1",
    "prompt": "A cute baby sea otter",
    "n": 1,
    "size": "1024x1024" # 1024x1024 (square), 1536x1024 (landscape), 1024x1536 (portrait), or auto (default value)
  }'
```

请把 openai 的通用逻辑放到 @src/openai.rs 中。命令行逻辑放到 @src/imgen.rs 中。

CLI usage:

```bash
imgen <yaml_file>
```
