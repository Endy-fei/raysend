# RaySend · 光传

**用光传文件。** 两台设备、一块屏幕、一颗摄像头，不需要 Wi-Fi、蓝牙或数据线。

[打开网站](https://raysend.myconjure.com) · [Releases](https://github.com/Endy-fei/raysend/releases) · [GitHub](https://github.com/Endy-fei/raysend)

[![Rust](https://img.shields.io/badge/Rust-1.80+-orange.svg)](https://www.rust-lang.org/)
[![WASM](https://img.shields.io/badge/target-wasm32--unknown--unknown-654FF0.svg)](https://webassembly.org/)
[![Dioxus](https://img.shields.io/badge/UI-Dioxus%200.7-00A8E8.svg)](https://dioxuslabs.com/)
[![GitHub Pages](https://img.shields.io/badge/demo-raysend.myconjure.com-2ea44f.svg)](https://raysend.myconjure.com)

![demo](demo.gif)

RaySend（光传）是一个跑在浏览器里的离线文件传输工具。发送端把文件压成动画二维码播放，接收端用摄像头扫码还原。全程没有账号、没有后台、没有上传：压缩、编码、扫描、校验、解压都在本机完成。

适合这些场景：

- 两台电脑互不联网，仍要把一份文档、密钥或配置送过去
- 隔离网、会议室、机房等不能插 U 盘、也不能连局域网的环境
- 手机和电脑之间临时传一个小文件，不想开热点、不想装 IM

---

## 目录

- [特点](#特点)
- [快速开始](#快速开始)
- [使用说明](#使用说明)
- [工作原理](#工作原理)
- [传输协议](#传输协议)
- [性能](#性能)
- [隐私与安全](#隐私与安全)
- [浏览器支持](#浏览器支持)
- [本地开发](#本地开发)
- [测试](#测试)
- [项目结构](#项目结构)
- [自行部署](#自行部署)
- [Windows 安装包](#windows-安装包)
- [常见问题](#常见问题)
- [贡献](#贡献)
- [技术栈](#技术栈)

---

## 特点

| | |
| --- | --- |
| **空气墙可用** | 通道是可见光，不依赖局域网、蓝牙、USB 或云存储 |
| **喷泉码** | RaptorQ 编码。漏扫、花屏、扫到重复帧都没关系，凑够符号即可还原 |
| **当场画码** | Canvas 即时生成二维码，不会预先堆几百张图把内存打满 |
| **1×1 / 2×2** | 手机默认单码；屏幕宽度 ≥ 720px 时默认 2×2，吞吐大约 4 倍 |
| **浏览器内压缩** | Brotli（质量 5）。文本、源码、文档体积通常会明显下降 |
| **完整性校验** | 压缩结果用 SHA-1 校验，对不上不会交付文件 |
| **中英界面** | 跟随浏览器语言，可随时切换，偏好保存在本机 |
| **深浅色** | 跟随系统主题，可手动切换 |
| **纯静态站点** | 无服务器。GitHub Pages 托管，也可以自己放到任意静态目录 |

单文件上限 **20 MB**。空文件不能发送。

---

## 快速开始

1. 发送端打开 [raysend.myconjure.com](https://raysend.myconjure.com)，选择或拖入文件。
2. 等待压缩完成，屏幕开始播放二维码。
3. 接收端打开同一网站，允许摄像头，对准发送端屏幕。
4. 进度走满后自动校验、解压并下载。

摄像头需要 **HTTPS**（或 `localhost`）。公网站点已经是 HTTPS；本地开发时 `dx serve` 提供的地址也可以用。

想确认文件没有离开本机：选文件前关掉 Wi-Fi / 蜂窝数据即可。站点本身不发起上传请求。

---

## 使用说明

### 发送

1. 打开 **发送** 页，点击虚线区域选文件，或把文件拖进去。
2. 浏览器读取文件 → Brotli 压缩 → 生成喷泉码。状态栏会显示当前步骤。
3. 进入播放页后可以：
   - **播放 / 暂停**
   - **调速度**（默认约 12 fps）
   - **单码 ↔ 2×2 宫格**
   - **返回** 重新选文件

2×2 适合笔记本或显示器：四个码同时播，接收端扫到任意足够帧即可，不必四个码都盯死。手机屏幕小，建议保持单码，把码尽量铺满、亮度拉高。

### 接收

1. 打开 **接收** 页，点 **开启相机**。
2. 对准发送端二维码。状态会从「对准」变为扫描进度。
3. 点按预览画面可在前后摄像头之间切换。
4. 完成后会响一声提示音，并触发下载。可用 **再次下载** 再保存一次。

接收端不必从第一帧开始扫，也不必按顺序扫。喷泉码的设计就是：任意足够多的互异符号都能还原。

### 实用建议

- 发送端全屏、提高屏幕亮度、关掉屏保。
- 接收端拿稳，避免强反光和过曝。
- 已经压缩过的文件（zip、jpg、mp4、pdf 等）几乎压不动，耗时接近上限估算。
- 纯文本、JSON、源码、CSV 往往压得很多，实际耗时会短不少。
- 传大文件时尽量用 2×2 + 较近距离，不要靠提高 fps 硬扛：扫不稳会浪费帧。

---

## 工作原理

```mermaid
flowchart LR
  A[读取文件] --> B[Brotli 压缩]
  B --> C[SHA-1]
  C --> D[RaptorQ 喷泉码]
  D --> E[协议封帧]
  E --> F[Canvas 二维码]
  F --> G[摄像头扫描]
  G --> H[解码符号]
  H --> I[凑够后还原]
  I --> J[校验哈希]
  J --> K[解压]
  K --> L[本地下载]
```

发送端循环输出两类帧：

- **元数据帧**：文件名、原始长度、压缩数据的 SHA-1、RaptorQ 的 OTI（Object Transmission Information）
- **数据帧**：RaptorQ 修复包。发送端按块轮转不断产生新包，而不是把源块按固定顺序重放

元数据会在开头连发两帧，之后大约每 8 帧插一次，所以接收端中途加入也能拿到文件名和编码器参数。

接收端用 `quircs` 从摄像头画面里找出二维码，解析协议，把符号喂给 RaptorQ 解码器。解码成功后再校验哈希、解压，最后用 Blob 触发浏览器下载，并带上按扩展名猜测的 MIME 类型。

二维码为 **QR Version 20、纠错等级 M**，在 Canvas 上按像素绘制，带 4 模块静区。符号载荷上限 **640 字节**，保证能放进该规格的二维码。

---

## 传输协议

帧均为二进制，便于塞进二维码的 byte 模式。

| 偏移 | 内容 |
| --- | --- |
| 0–1 | 魔数 `QT` |
| 2 | 类型：`M` 元数据 / `D` 数据 |

**元数据 `M`**

| 字段 | 长度 | 说明 |
| --- | --- | --- |
| 原始文件长度 | 4 字节，大端 `u32` | 解压后的字节数 |
| 哈希 | 20 字节 | 压缩结果的 SHA-1 |
| OTI | 12 字节 | RaptorQ 解码所需配置 |
| 文件名 | 剩余字节 | UTF-8，最长 180 字节 |

**数据 `D`**

魔数和类型之后是完整的 RaptorQ 编码包（含包头）。接收端在拿到元数据之前会把数据帧暂存，元数据到达后再一次性喂给解码器。

无法识别的字节会被忽略，不会中断传输。

---

## 性能

吞吐取决于屏幕大小、相机素质、距离和宫格，而不是网速。

| 模式 | 经验吞吐 | 已压缩的 20 MB 大约耗时 |
| --- | --- | --- |
| 单码 | 约 8–15 KB/s | 约 15–40 分钟 |
| 2×2 | 大约 4 倍 | 大约四分之一 |

播放器按 `符号大小 × fps × 宫格数 × 0.7` 估算剩余时间，0.7 用来覆盖漏扫和元数据开销。界面上的进度来自「互异符号数 / 预计所需符号数」，不是按时间轴假装前进。

解码器侧为 RaptorQ 预留最多约 **48 MB** 工作内存，与 20 MB 文件上限匹配。

---

## 隐私与安全

RaySend 的目标是：**文件不要经过任何你看不见的机器。**

- 没有登录、没有遥测、没有后端 API。
- 站点是静态 HTML + WASM，由 GitHub Pages 提供。
- 传输介质是屏幕上的可见光。旁边的人如果能看到屏幕，理论上也能扫——这和把文件举到别人眼前是同一类风险。
- 哈希用于发现传输出错，不是保密手段。需要保密时，请先在本机加密再发送。
- 语言偏好存在 `localStorage` 键 `raysend.lang`，不会上传。
- 摄像头仅在接收页、你点击开启后使用；离开接收或点停止会关掉轨道。

这不是端到端加密通讯工具，也不是网盘。它解决的是「两台机器之间没有电的连接」这一个问题。

---

## 浏览器支持

需要：

- WebAssembly
- Camera / `getUserMedia`（接收）
- Canvas 2D
- Blob 下载

建议使用较新的 **Chrome、Edge、Safari、Firefox**。iOS Safari 可以接收，但必须在 HTTPS 下授权相机。部分浏览器的隐私模式会限制 `localStorage`，语言切换可能无法记住，不影响传输。

桌面发送 + 手机接收是最常见组合。两台电脑也可以：一台全屏播码，另一台用摄像头或外接摄像头对准。

---

## 本地开发

### 环境

- Rust（stable）
- 目标三元组 [`wasm32-unknown-unknown`](https://doc.rust-lang.org/rustc/platform-support/wasm32-unknown-unknown.html)

  ```bash
  rustup target add wasm32-unknown-unknown
  ```

- [Dioxus CLI 0.7.x](https://dioxuslabs.com/learn/0.7/getting_started)

  ```bash
  cargo install dioxus-cli --version 0.7.10 --locked
  ```

### 运行

```bash
git clone https://github.com/Endy-fei/raysend.git
cd raysend
dx serve --platform web
```

终端会打印本地地址，用浏览器打开即可。改 Rust / CSS 后 CLI 会重新编译 WASM。

`build.rs` 会在首次构建时把 GitHub Mark 图标下载到 `public/assets/images/`（该目录已加入 `.gitignore`）。离线构建前请确保该文件已存在，或允许构建脚本访问网络。

### 生产构建

```bash
dx bundle --release --platform web --out-dir dist
```

静态资源在 `dist/public/`。可以丢进任何静态服务器、对象存储或 GitHub Pages。

---

## 测试

协议、压缩和喷泉码还原不依赖浏览器，可直接：

```bash
cargo test
```

覆盖内容包括：

- 元数据 / 数据帧编解码，以及垃圾输入拒绝
- Brotli 往返
- 丢包条件下 RaptorQ 仍能还原
- 元数据后到时，暂存的数据帧仍能被消化
- QR Version 20 + ECC M 放得下协议载荷
- 扫描管线从喷泉帧重建文件

WASM UI 需要在浏览器里手测：选文件、播码、授权相机、前后摄像头、中英切换、深浅色。

---

## 项目结构

```text
raysend/
├── src/
│   ├── main.rs              # 应用壳：发送 / 接收页、主题、语言
│   ├── lib.rs               # 全局状态与端到端测试
│   ├── compress.rs          # Brotli
│   ├── fountain.rs          # RaptorQ 发送 / 接收
│   ├── protocol.rs          # QT 二进制帧
│   ├── i18n.rs              # 中英文案
│   ├── utils.rs             # SHA-1、日志、字节格式化
│   ├── send/
│   │   ├── mod.rs           # 选文件、播放页
│   │   └── encoder/         # Canvas 画二维码
│   └── receive/
│       ├── mod.rs           # 相机、下载、提示音
│       └── decoder.rs       # 画面 → 二维码 → 喷泉解码
├── public/
│   ├── app.css
│   ├── favicon.svg
│   └── CNAME
├── index.html
├── Dioxus.toml
├── build.rs
└── .github/workflows/
    ├── pages.yml            # 推送 master 后发布网站
    └── release.yml          # 手动打 Windows MSI
```

---

## 自行部署

推送到 `master` 后，GitHub Actions 会执行 `dx bundle`，并把 `dist/public` 发布到 `gh-pages`。当前绑定域名为 [raysend.myconjure.com](https://raysend.myconjure.com)。

若使用自己的域名：

1. 把 `public/CNAME` 和 `.github/workflows/pages.yml` 里的 `cname` 改成你的域名。
2. DNS 增加 **CNAME**，指向 `endy-fei.github.io`（或你的 `用户名.github.io`）。
3. 在仓库 Settings → Pages 中确认源分支为 `gh-pages`。

也可以跳过 GitHub Pages，把 `dx bundle` 的产物放到 Nginx、Caddy、Cloudflare Pages、对象存储静态网站等任意位置。记住：**接收端必须是 HTTPS**，否则浏览器不会开放摄像头。

---

## Windows 安装包

在线版已经能用。若需要离线桌面壳（把站点打成本地窗口）：

1. 打开 GitHub Actions 里的 **release** 工作流。
2. 手动 `Run workflow`。
3. 完成后在 [Releases](https://github.com/Endy-fei/raysend/releases) 下载 `RaySend.msi`。

该工作流使用 [Pake](https://github.com/tw93/Pake) 包装 `dx bundle` 的产物，带本地文件拖放。它不会上传你的文件。

---

## 常见问题

**为什么比网盘慢这么多？**  
可见光二维码的带宽大约是十几 KB/s 量级，这是媒介决定的。RaySend 换的是「不需要网络和线」，不是速度。

**扫了很久进度不动？**  
先确认发送端在播放、码没有被窗口挡住。调低 fps、改回单码、拉近距离、提高亮度。接收端进度看的是互异符号，重复扫同一帧不会涨。

**相机打不开？**  
检查是否 HTTPS、是否拒绝过权限、是否被其他应用占用。iOS 上必须用 Safari 或支持 `getUserMedia` 的浏览器，并在系统设置里允许相机。

**文件下下来是坏的？**  
正常路径会先校验哈希再解压。如果浏览器下载被杀毒软件改写，请对比文件大小；也可以点再次下载。

**能传文件夹吗？**  
目前是单文件。请先自行打包成 zip 再发。注意 zip 已经压缩，时间会接近「按体积估算」的上限。

**最大为什么是 20 MB？**  
二维码通道慢，再大就不实用；同时 RaptorQ 解码也要占内存。20 MB 是体验和资源之间的折中。

**语言切换没记住？**  
部分隐私模式禁用 `localStorage`。刷新后会按浏览器语言重新检测。

---

## 贡献

Issue 和 Pull Request 都欢迎。改协议或编码参数时，请补 `cargo test`，并说明是否还与现有已发布站点兼容（当前协议魔数为 `QT`，现场版本互传即可）。

建议的改动方向：

- 更大屏的宫格或自适应布局
- 更稳的摄像头对焦 / 取景引导
- 更多语言
- 在不牺牲空气墙前提的前提下缩短大文件时间

---

## 技术栈

| 部分 | 选用 |
| --- | --- |
| 语言 / 运行时 | Rust → WebAssembly |
| UI | [Dioxus](https://dioxuslabs.com/) 0.7 |
| 喷泉码 | [raptorq](https://github.com/cberner/raptorq) 2.0（RFC 6330） |
| 压缩 | [brotli](https://github.com/dropbox/rust-brotli) |
| 二维码生成 | [qrcode](https://crates.io/crates/qrcode) |
| 二维码识别 | [quircs](https://crates.io/crates/quircs) |
| 哈希 | SHA-1（完整性，非密码学用途） |

---

两台互不联网的设备，一块屏幕，一颗摄像头。文件从这边走到那边。
