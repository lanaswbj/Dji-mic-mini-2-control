# Dji-mic-mini-2-control

面向 Windows 的大疆麦克风控制工具，基于开源 DJI Mic USB 控制项目扩展，重点适配 DJI Mic Mini 2 双发射器使用场景。

应用提供中文界面、接收器与发射器状态读取、双发射器独立设置。

> 本项目为非官方社区项目，与 DJI（大疆创新）没有隶属或授权关系。修改设备参数前请确认设备型号和固件版本。

## 主要功能

- 中文 Windows 图形界面。
- 读取接收器、两个发射器的连接状态、序列号、电量和固件信息。
- 双发射器独立控制降噪、人声音色等支持的参数。
- 修复双发射器同时连接时部分开关状态反复跳动的问题。
- 支持 DJI Mic Mini 2 多色前盖外观显示。
- 系统托盘运行、开机自启和设备快捷控制。
- 一键安装接收器 USB 控制接口所需的 WinUSB 驱动（自动下载并校验官方签名的 Zadig，安装完成后自动清理，无需手动操作驱动程序）。
- 接收器配对键按下检测（通过 Win32 Raw Input API 读取 HID 报文），以及麦克风外壳敲击手势检测（单击/双击，基于声音特征训练的本地分类模型，见下方"麦克风敲击检测"）。两者目前在界面的"接收器快捷键"页作为检测状态展示，尚未开放绑定到具体按键/动作。
  > 按下配对键时，接收器仍会触发 Windows 系统音量调节这一副作用，目前没有可靠的抑制方式（已知 `RIDEV_NOLEGACY` 标志位对该 HID usage page 无效）。

### 麦克风敲击检测

轻敲麦克风外壳可以被识别为单击或双击手势（3 下及以上会归为双击）。识别流程：

1. 提取每个音频帧的电平特征（peak/RMS/相对基线比值）与 4 段 Goertzel 频谱特征（频谱质心、高低频能量比），刻画声音的"音色"而不只是响度。
2. 用 [Silero VAD](https://github.com/snakers4/silero-vad) 语音活动检测模型判断当前是否有人在说话，说话时暂停敲击识别，避免尖锐辅音、感叹词被误判成敲击。
3. 用频谱通量起始点检测（spectral flux onset novelty，音乐信息检索领域的成熟瞬态检测技术）衡量声音瞬态的突然程度。
4. 以上特征输入一个小型本地训练的神经网络（1 隐藏层，softmax 三分类：无/指甲敲击/指腹敲击），替代早期从参考项目移植但在本机硬件上完全不生效的固定阈值方案。
5. 叠加固定的电平硬阈值、防抖和多候选确认机制，进一步降低说话、衣物摩擦、敲击桌面等环境噪声的误触发概率。

`test-tools/detect-test` 是一个独立于主工程之外的命令行调试/训练工具，用于采集真实敲击样本（`cargo run -- collect`）并重新训练模型（`cargo run -- train`）；模型效果需要针对具体硬件调整时可以从这里入手。

> 接收器按键快捷键映射（短按接收器映射为 `Fn + Control`）在应用界面中保留了入口，但尚未提供 Windows 实现，当前会显示为不可用。
>
> 原 macOS 版本的 Voice Comfort 实时人声处理、应用内音频输入输出设备切换功能依赖 macOS 专属的 CoreAudio 接口，并且 Voice Comfort 依赖 BlackHole 等虚拟音频设备完成系统音频路由。这两项功能在本 Windows 版本中已移除。

## 界面与技术栈

- 前端：Svelte 5 + Vite
- 桌面框架：Tauri 2
- 设备通信：Rust + USB/HID（`cpal` 音频采集，Win32 Raw Input API）
- 麦克风敲击检测：本地训练的小型神经网络 + [Silero VAD](https://github.com/snakers4/silero-vad)（经 `voice_activity_detector` crate 调用）+ `microdsp`（频谱通量起始点检测）
- 支持平台：Windows；上游协议、设备层和 CLI 仍保留跨平台结构（Linux 打包文件同样保留）

## 开发运行

环境要求：

- Rust 1.81 或更高版本（含 Windows MSVC 工具链）
- Node.js 18 或更高版本
- npm

安装前端依赖并启动开发版：

```bash
cd gui
npm install
npm run tauri dev
```

构建 Windows 应用：

```powershell
.\build-release.ps1
```

该脚本会在 `Release\windows\` 下产出便携版 `DJI Mic Control.exe` 和 NSIS 安装包。也可以手动执行：

```bash
cd gui
npx tauri build --bundles nsis
```

命令行工具示例：

```bash
cargo run -p cli -- list
cargo run -p cli -- status --json
cargo run -p cli -- set noise-cancel strong
```

协议研究记录见 [PROTOCOL.md](PROTOCOL.md)。

## 项目结构

```text
crates/protocol                  DJI Mic 协议、数据帧和设备模型
crates/device                    USB 设备发现与通信
crates/cli                       命令行控制工具
gui                              Tauri + Svelte 图形界面
test-tools/detect-test           麦克风敲击检测模型的独立调试/采集/训练工具
packaging                        Linux 打包与 udev 文件
```

## 致谢与来源

本项目是在以下作者的公开项目和研究基础上继续开发的：

1. [ShadowBitBasher](https://github.com/ShadowBitBasher) — [DJI-Mic-Control](https://github.com/ShadowBitBasher/DJI-Mic-Control)：提供核心 USB 控制协议、Rust 设备层、CLI 和跨平台 GUI 基础。
2. [hueyluox](https://github.com/hueyluox) — [dji-mic-command](https://github.com/hueyluox/dji-mic-command)：提供 DJI 接收器按键事件与快捷键映射方面的研究参考。
3. [Jayaway](https://github.com/Jayaway) — [Vibe-Coding-for-DJI-Mic](https://github.com/Jayaway/Vibe-Coding-for-DJI-Mic)：提供 DJI Mic 快捷操作和语音工作流实验的思路参考。

感谢上述作者公开代码和研究过程。各上游项目的版权及许可归原作者所有；复用或分发时请同时遵守其各自仓库中的许可证。

麦克风敲击检测功能使用了以下开源模型/技术：

- [Silero VAD](https://github.com/snakers4/silero-vad)（[snakers4](https://github.com/snakers4) 等）—— 用于判断当前是否有人声，从而在说话时暂停敲击识别；通过 [`voice_activity_detector`](https://github.com/nkeenan38/voice_activity_detector) Rust 封装调用。
- [`microdsp`](https://github.com/stuffmatic/microdsp)（[stuffmatic](https://github.com/stuffmatic)）—— 提供频谱通量起始点（spectral flux onset）检测算法实现。

敲击手势本身的分类模型（指甲/指腹/无敲击）是基于以上信号在本机真实硬件上采集样本、独立训练得到的本地小型神经网络，非第三方预训练模型。

## 许可证

当前仓库沿用上游项目附带的 [LICENSE](LICENSE)。第三方依赖及参考项目保留各自许可证和版权声明。
