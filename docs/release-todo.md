# TODO: GitHub Release（可下载版本）

> 状态：**未开始**，2026-07-28 调研完成后决定暂缓。
> 目标：在 GitHub Releases 放可下载的安装包，让用户不用自己 build。

## ⚠️ 先读这个：现有 bundle 产物是坏的

`src-tauri/Cargo.toml` 里 `whisper-rs` 开了 `cuda` feature 且**不是 optional**，
导致编译出的 exe 对 CUDA 有 **load-time 硬依赖**（不是 runtime 才去找）。

已验证（`dumpbin /DEPENDENTS target/release/voice-dictation.exe`）：

```
cublas64_13.dll   ← 硬性 import
```

没装 CUDA 13 runtime 的机器上 Windows loader 直接失败，App 连启动都启动不了
（用户只看到 0xc000007b 之类的弹窗）。**所以不要直接把现有的 NSIS installer 传上去。**

现有产物实测：

| 产物 | 大小 | 能否运行 |
|---|---|---|
| `src-tauri/target/release/bundle/nsis/Voice Dictation_0.1.0_x64-setup.exe` | 39 MB | ❌ 缺 CUDA DLL |
| `src-tauri/target/release/bundle/msi/Voice Dictation_0.1.0_x64_en-US.msi` | 0.8 MB | ❌ 同上 |
| `D:\Tools\VoiceDictation\`（手动部署，含 CUDA DLL） | **547 MB** | ✅ 但需 NVIDIA GPU |

547 MB 的构成：`cublasLt64_13.dll` 434 MB + `cublas64_13.dll` 49 MB +
`cudart64_13.dll` 0.5 MB + exe 63 MB。那个 434 MB 的没法裁。

核心取舍：**40 MB 但跑不起来** vs **550 MB 但挑显卡**。两个都不适合当 public release 主产物。

## 方案：主产物走 CPU + 云端，CUDA 版做次要产物

### 主产物：`Voice-Dictation-x.y.z-x64-setup.exe`（~45 MB）
- 不开 cuda feature → CPU whisper + Groq 云端转录
- Win10/11 通吃，不挑显卡，无需 CUDA
- **代码一行都不用改**：`src-tauri/src/transcribe/local.rs:39` 的
  `params.use_gpu(true)` 在无 cuda 编译下是安全的 no-op
- 本地模型本来就是 runtime 下载（`src-tauri/models/` 已在 gitignore），
  不会撑大 installer
- 速度：小模型 CPU 听写够用；要快就用 Groq 云端（本来就比本地 CUDA 快）

### 次要产物：`VoiceDictation-x.y.z-cuda-x64.zip`（~550 MB）
- 绿色版 zip，不做 installer，直接解压运行
- 说明写清「需 NVIDIA GPU / 550 MB / 本地转录更快」
- NVIDIA EULA 允许 redistribute 这几个 runtime DLL，无法律问题

## 步骤

- [ ] `src-tauri/Cargo.toml`：把 whisper-rs 改成
      `whisper-rs = { version = "0.16", default-features = false }`，
      并加 `[features]` 段：`cuda = ["whisper-rs/cuda"]`
- [ ] 验证默认（CPU）build 能编译且能启动，本地转录仍可用（只是慢）
- [ ] 验证 `cargo build --release --features cuda` 仍与现状一致
- [ ] 版本号对齐：`package.json` 与 `src-tauri/tauri.conf.json`（目前都是 `0.1.0`）
- [ ] 打 tag `v0.1.0`
- [ ] 本地 build 两个产物，`gh release create v0.1.0 ...` 上传
      （`gh` 已装：`C:\Program Files\GitHub CLI\gh.exe`）
- [ ] README 加 Download 段落，说明两个版本怎么选
- [ ] README 说明未签名会触发 SmartScreen「未知发布者」警告及如何绕过

## 明确决定不做

- **GitHub Actions 自动出 release**：CPU 版在 CI 上好搞，但 CUDA 版要在 runner
  上装 CUDA toolkit，编译 whisper.cpp 十几分钟起跳且容易挂。单人项目本地
  build + 一条 `gh release create` 就够，发得勤了再自动化。
- **代码签名证书**：一年几百刀，现阶段 README 写说明即可。
- **Tauri updater plugin**（自动更新）：等有真实用户再说。

## 相关

- [toolchain-setup.md](toolchain-setup.md) — 改 whisper-rs / CUDA / bindgen 前必读
