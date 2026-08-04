# TGSConvert

一个把 Telegram TGS 贴纸（Lottie 动画）转换为透明视频/动画格式的 Rust CLI。

## 功能

- TGS / Lottie JSON / gzip JSON 输入
- 输出格式：
  - WebM（VP9 + alpha，`tgs-webm`，默认命令）
  - MOV（Apple ProRes 4444 + alpha，`tgs-webm mov`）
  - WebP（动画无损 + alpha，`tgs-webm webp`）
  - GIF（gifski，仅支持 1/2/4/5/10/20/25/50 FPS，`tgs-webm gif`）
- Telegram 贴纸包下载（`tgs-webm telegram-download`，支持 `t.me/addstickers` 与 `t.me/addemoji`）
- 并行渲染：多个独立 rlottie worker 分帧渲染，帧序列写入临时目录
- 参数：`--fps`（GIF 上限 50）、`--quality`、`--width/--height`、`--play-speed`、`--rotation`、`--flip-horizontal/--flip-vertical`、`--threads`、`--ffmpeg`

## 依赖

运行时：

- FFmpeg（WebM / MOV 编码）
- gifski（GIF 编码）
- img2webp（WebP 编码，Homebrew `webp` 公式提供）

构建时：

- Rust（stable，含 `i686-pc-windows-gnu` target）
- CMake 与 git（rlottie-sys 构建 vendored rlottie）
- zig（Windows x86 交叉编译）
- llvm-ar 或 mingw-w64（归档器；zig 0.16 的 `zig ar` 有缺陷时的替代）

## 构建

macOS 原生：

```sh
cd tgs-webm
cargo build --release
```

Windows x86-64（64 位）交叉编译，使用 zig 作为 C/C++ 工具链与链接器：

```sh
zig build
# 产物：zig-out/bin/tgs-webm.exe
```

## 用法

```sh
# TGS -> WebM
tgs-webm 测试.tgs --output out.webm --fps 60 --quality 100 --threads 8

# TGS -> ProRes 4444 MOV
tgs-webm mov 测试.tgs --output out.mov --fps 240 --quality 100

# TGS -> WebP
tgs-webm webp 测试.tgs --output out.webp --fps 60 --quality 100

# TGS -> GIF（50 FPS 上限，帧延迟为 10ms 整数倍）
tgs-webm gif 测试.tgs --output out.gif --fps 50 --quality 100

# 下载完整 Telegram 贴纸包
tgs-webm telegram-download https://t.me/addstickers/SomePack --output-dir ./packs/SomePack
```

## 说明

- WebP 计时按帧显式写入毫秒延迟（60 FPS 使用 16/17/17ms 循环，总时长精确），不复用原 C# 项目的错误计时器。
- GIF 帧延迟以 10ms 为单位，因此帧率被限制在 50 FPS 以内且必须能整除 1000ms。
- Telegram bot token 已按需求硬编码进二进制。

## CI

`.github/workflows/ci.yml` 在 macOS runner 上用 zig 交叉编译 x86 二进制，并在 Windows runner 上安装 FFmpeg 后实际执行转换验证。
