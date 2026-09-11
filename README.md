# RaySend · 光传

两台设备、一块屏幕、一颗摄像头。文件走光，不走网。

[打开网站](https://raysend.myconjure.com) · [Windows 安装包](https://github.com/Endy-fei/raysend/releases) · [GitHub](https://github.com/Endy-fei/raysend)

![demo](demo.gif)

适合搜这些词的人：二维码传文件、离线传文件、空气墙传文件、摄像头传文件、QR file transfer、air-gapped QR。

没有账号，没有后台。压缩、编码、扫描、还原都在浏览器里完成。

## 特点

- **空气墙可用**：不靠 Wi-Fi、蓝牙或数据线，光路就是通道
- **喷泉码**：RaptorQ 编码，漏扫几帧不用从头再来
- **当场画码**：Canvas 即时生成二维码，不会预先堆几百张图把内存打满
- **2×2 宫格**：大屏大约 4 倍吞吐；手机默认单码
- **中英界面**：跟随浏览器语言，可随时切换
- **最大 20 MB**：文本和文档压缩后通常会快很多

单码大约 8–15 KB/s。已经压过的 20 MB 文件大约 15–40 分钟。

## 使用

1. 发送端打开网站，选文件，等二维码开始播放。
2. 接收端打开同一网站，允许摄像头，对准屏幕。
3. 进度走满后自动校验、解压并下载。点画面可切换前后摄像头。

选文件前可以关掉 Wi-Fi，用来确认没有流量离开本机。

## 本地开发

需要 Rust、`wasm32-unknown-unknown` 和 [Dioxus CLI](https://dioxuslabs.com/learn/0.7/getting_started)。

```bash
dx serve --platform web
```

`cargo test` 覆盖协议、压缩和喷泉码还原。

```bash
dx bundle --release --platform web --out-dir dist
```

产物在 `dist/public/`。

## 发布

推送到 `master` 后自动发布到 [raysend.myconjure.com](https://raysend.myconjure.com)。

DNS：`raysend.myconjure.com` CNAME 到 `endy-fei.github.io`。

Windows 安装包：Actions 里手动跑 `release`。

## 依赖

编码使用 [cberner/raptorq](https://github.com/cberner/raptorq)，界面使用 [Dioxus](https://dioxuslabs.com/)。
