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

1. 提取每个音频帧的电平特征（peak/RMS/相对基线比值）与 12 段对数间隔 Goertzel 频谱特征（150Hz–8kHz，刻画频谱轮廓/质心/高低频能量比/衰减点/平坦度），以及攻击位置、前后段能量比、帧间差分等时序特征，刻画声音的"音色"和"形状"而不只是响度。
2. 用 [earshot](https://github.com/pykeio/earshot)（纯 Rust 神经网络语音活动检测模型）判断当前是否有人在说话，说话时暂停敲击识别，避免尖锐辅音、感叹词被误判成敲击。
3. 用频谱通量起始点检测（spectral flux onset novelty，音乐信息检索领域的成熟瞬态检测技术）衡量声音瞬态的突然程度。
4. 以上特征输入一个小型本地训练的神经网络：对 12 段频谱额外做了一层小型一维卷积（提取频段间的邻接关系）+ 全局最大池化，再与其余标量特征拼接后输入 1 隐藏层、softmax 二分类（敲击/非敲击），替代早期从参考项目移植但在本机硬件上完全不生效的固定阈值方案。
5. 叠加（比早期版本宽松很多的）电平硬阈值、防抖和多候选确认机制，进一步降低说话、衣物摩擦、敲击桌面等环境噪声的误触发概率。
6. 模型支持热加载：应用运行时每隔几秒检测一次模型文件是否有更新，重新训练或收到识别反馈后无需重启即可生效。界面"接收器快捷键"页提供"误判撤销"/"漏判补报"两个反馈入口，点击后会在本地用最近的录音自动做一次小规模增量训练（带安全校验，效果变差会自动放弃这次更新），并可以随时回滚到上一个模型或恢复出厂模型。

`test-tools/detect-test` 是一个独立于主工程之外的命令行调试/训练工具，用于采集真实敲击样本（`cargo run -- collect`）并重新训练模型（`cargo run -- train`）；模型效果需要针对具体硬件调整时可以从这里入手。模型的权重格式、前向推理和训练逻辑统一收敛到共享的 `crates/tap-model`，应用运行时加载和离线训练用的是完全相同的一套代码。

> 接收器按键快捷键映射（短按接收器映射为 `Fn + Control`）在应用界面中保留了入口，但尚未提供 Windows 实现，当前会显示为不可用。
>
> 原 macOS 版本的 Voice Comfort 实时人声处理、应用内音频输入输出设备切换功能依赖 macOS 专属的 CoreAudio 接口，并且 Voice Comfort 依赖 BlackHole 等虚拟音频设备完成系统音频路由。这两项功能在本 Windows 版本中已移除。

## 界面与技术栈

- 前端：Svelte 5 + Vite
- 桌面框架：Tauri 2
- 设备通信：Rust + USB/HID（`cpal` 音频采集，Win32 Raw Input API）
- 麦克风敲击检测：本地训练的小型神经网络（含 1D 卷积）+ [earshot](https://github.com/pykeio/earshot)（纯 Rust VAD）+ `microdsp`（频谱通量起始点检测），权重/推理/训练统一放在共享的 `crates/tap-model`
- 支持平台：Windows；上游协议、设备层和 CLI 仍保留跨平台结构（Linux 打包文件同样保留）

## 编译指南

### 环境要求

- Rust 1.81 或更高版本，并安装好 **Windows MSVC 工具链**（Visual Studio Build Tools，需包含"使用 C++ 的桌面开发"负载，提供 `link.exe`）。普通 PowerShell/Git Bash 会话默认不在 `PATH` 上暴露这套工具链，需要先执行一次
  `"C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64`
  （路径按实际安装位置调整）来加载环境变量，再在同一个终端里跑 `cargo build`/`cargo run`。
  > 如果用的是 Git Bash：它自带的 coreutils `link` 命令会抢在 MSVC 的 `link.exe` 之前被 `PATH` 命中，导致链接失败且报错信息很难看出真实原因。建议在纯 PowerShell 会话里执行编译相关命令。
- Node.js 18 或更高版本，以及 npm（随 Node.js 一起安装）。
- 可选：[cargo-deny](https://github.com/EmbarkStudios/cargo-deny)，用于校验依赖许可证（`cargo deny check`，规则见 [deny.toml](deny.toml)）。

首次克隆仓库后不需要额外初始化步骤——`cargo build`/`npm install` 会按需拉取所有依赖。

### 快速检查（改动代码后先跑这个）

```bash
cargo check --workspace   # 只做类型检查，比完整构建快很多
cargo test --workspace    # 跑 protocol/tap-model 等 crate 的单元测试
```

### 开发模式运行（带热重载）

```bash
cd gui
npm install         # 首次运行或依赖变更后执行
npm run tauri dev    # 同时启动 Rust 后端和 Svelte 前端，支持热重载、可直接访问 USB 设备
```

`npm run dev` 只启动 Vite 前端开发服务器，不含 Tauri 后端，无法访问真实设备，仅适合单独调整界面样式时使用。

### 生产环境构建

推荐直接在仓库根目录用 PowerShell 执行打包脚本，而不是手动调用 `tauri build`：

```powershell
.\build-release.ps1
```

该脚本会调用 `npx tauri build --bundles nsis`，并额外做一次"路径隐私"清理：通过 `--remap-path-prefix` 把编译期用户主目录/用户名/主机名从最终二进制的 panic/backtrace 字符串里移除（`strip` 本身不会清理这些），构建完成后会重新扫描产物，只要发现残留路径就会让构建直接失败，而不是悄悄产出一个带有构建者本机信息的安装包。

产物输出在 `Release\windows\` 目录下：

- `DJI Mic Control.exe` —— 免安装的便携版，运行前需要系统已装好 WebView2 运行时（大多数 Windows 10/11 已自带）。
- `DJI Mic Control_<版本号>_x64-setup.exe` —— NSIS 安装包，会注册到"添加或删除程序"和开始菜单，并在安装时自动补装 WebView2 运行时。

如果只想快速验证打包流程、不需要路径隐私清理，也可以手动执行（注意这样产出的二进制可能带有构建机器的本地路径信息，不建议用于对外分发）：

```bash
cd gui
npx tauri build --bundles nsis
```

`build-release.sh` 是面向上游跨平台版本的 Linux/macOS 构建脚本，在本仓库（Windows-only 分支）里会直接拒绝在非 Windows 平台（尤其是 macOS）上执行。

### 命令行工具（djimic）

一次性命令：连接设备、执行一个动作、打印结果、退出，不提供持续监听。

```bash
cargo run -p cli -- list                       # 列出已连接设备
cargo run -p cli -- status --json              # 查看某设备完整状态（JSON 格式）
cargo run -p cli -- set noise-cancel strong     # 设置降噪模式
cargo run -p cli -- set voice-tone rich --tx 1  # 设置 1 号发射器的人声音色（仅 Mic Mini 2）
```

`--device <序列号>` 可以在连接了多台设备时指定目标；只连一台时可省略。协议层支持的具体设置项和取值见 `crates/protocol/src/models/` 下对应型号的源码。

### 麦克风敲击模型训练工具（可选，仅在需要重新训练检测模型时用到）

`test-tools/detect-test` 是独立于主工作区之外的一个命令行小工具，专门用来在不重建整个 Tauri 应用的情况下快速采集样本、迭代训练敲击检测模型：

```bash
cd test-tools/detect-test
cargo run --release -- collect             # 采集基础样本（约 4 分钟：安静/说话/大声说话/噪声/指甲敲击/指腹敲击几个阶段）
cargo run --release -- collect-extra       # 采集额外的难例负样本（约 2 分钟：按配对键、吹气）
cargo run --release -- collect-friction    # 采集摩擦噪声难例负样本（约 1 分钟：手指摩擦机身外壳）
cargo run --release -- train               # 用已采集的样本重新训练模型，热更新到当前正在运行的应用（约 3 秒内生效，无需重启）
cargo run --release -- train --bake-default  # 训练同时把新模型写入 crates/tap-model/default_model.json，成为全新安装的出厂默认模型
cargo run --release                        # 不带子命令：进入实时检测控制台，同时监听配对键，便于现场调参、不开 GUI 也能验证效果
```

一般使用者不需要用到这个工具——应用自带的出厂模型已经训练好，日常使用中还可以通过界面的"识别反馈"按钮做增量训练（见下方使用指南）。只有在打算彻底重新采集数据、更换硬件或调整模型结构时才需要用它。

协议逆向工程记录见 [PROTOCOL.md](PROTOCOL.md)。

## 使用指南

### 首次运行与驱动安装

1. 双击运行便携版 `DJI Mic Control.exe`，或从开始菜单启动安装版。应用默认最小化到系统托盘运行，托盘图标会实时反映设备连接和电量状态。
2. 插入 DJI Mic 接收器。如果 Windows 还没有为其"控制接口"（Interface 6 / MI_06）安装合适的驱动，主界面会显示"需要安装驱动"的提示卡片。
3. 点击"一键修复驱动"按钮：应用会下载并校验官方签名的 Zadig 驱动安装工具，在弹出的 Windows UAC 权限提示中选择"是"。
4. Zadig 打开后会预先选中正确的设备和驱动类型（WinUSB），确认界面上显示的设备名称包含 "Interface 6" / "MI_06" 字样后，点击 Zadig 内的 Install（或 Replace Driver）按钮完成安装。安装完成后 Zadig 会自动关闭，回到应用即可看到设备正常连接。
5. 之后正常插拔设备无需重复这一步骤。

### 主界面：设备与发射器设置

- 左侧设备列表显示接收器和已连接的发射器（Mic Mini 2 支持双发射器）。
- 选中发射器后可以查看/修改降噪模式、降噪开关、指示灯、人声音色（仅 Mic Mini 2）等支持的设置项，修改后立即通过 USB 下发到设备。
- 两个发射器的设置相互独立，同时连接时不会互相干扰（早期版本存在开关状态互相跳动的问题，已修复）。
- 托盘菜单里也提供了常用开关（降噪、指示灯）和"开机自启（后台）"选项，无需打开主窗口即可快速切换。关闭主窗口默认只是隐藏到托盘，真正退出需要点击托盘菜单里的"退出"。

### 接收器快捷键页

侧边栏"接收器快捷键"页面包含两部分：

- **配对键 / 敲击检测状态**：实时显示接收器配对键是否被按下，以及麦克风外壳敲击手势（单击/双击）的识别状态，方便验证硬件和识别是否正常工作。当前这两个信号尚未开放绑定到具体动作/快捷键，仅作状态展示。
- **识别反馈 · 增量训练**：如果敲击检测出现误判（没敲却触发了）或漏判（敲了但没反应），可以立即点击对应按钮告诉模型："刚才不是敲击（误判，撤销）" 或 "刚才敲了却没反应（漏判，补报）"。应用会在本地用最近几秒的录音自动做一次小规模增量训练，全程离线、无需重启，训练完成前后都有内置的安全校验，效果变差会自动放弃这次更新，不会让识别越用越差。这一页还能看到当前模型来源（出厂模型 / 本地全量训练 / 增量训练更新）和累计反馈条数，并提供"回滚到上一个模型"和"恢复出厂模型"两个按钮，误操作可以随时撤销。

### 全局 Pie 菜单

按下 `Ctrl+Alt+P`（任意应用内均可触发，全局热键）会在任务栏上方弹出一个类似 LG webOS 风格的扇形快捷菜单，用来在不切出当前工作场景的情况下快速执行一些操作（例如开启语音听写）：

- 方向键，或对着麦克风外壳做一次敲击手势，用来在菜单项之间移动高亮选择。
- 按 Enter，或按一下接收器的配对键，确认当前选中项（配对键在按下时也会同时被系统当成一次 Enter 按键处理，这是有意的设计，用来让配对键可以直接确认菜单，不需要额外接线）。
- 按 Escape，或让菜单失去焦点，取消并关闭菜单。
- 当 Claude Code 触发一个需要人工确认的权限请求，或弹出一个多选项问题（`AskUserQuestion`）时，这个菜单会自动切换显示成对应的真实选项内容，选中并确认即可直接把结果回传给 Claude Code，无需切回终端窗口操作。

### 命令行工具日常使用

不需要图形界面时，`djimic` 命令行工具可以单独用来查看或修改设置（见上方"编译指南"里的示例），适合写脚本或快速核对某个设置的当前值。

## 项目结构

```text
crates/protocol                  DJI Mic 协议、数据帧和设备模型
crates/device                    USB 设备发现与通信
crates/cli                       命令行控制工具
crates/tap-model                 敲击检测模型格式/推理/训练，被 gui 和 detect-test 共用
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

- [`earshot`](https://github.com/pykeio/earshot)（[pykeio](https://github.com/pykeio)）—— 纯 Rust 神经网络语音活动检测（VAD），用于判断当前是否有人声，从而在说话时暂停敲击识别。
- [`microdsp`](https://github.com/stuffmatic/microdsp)（[stuffmatic](https://github.com/stuffmatic)）—— 提供频谱通量起始点（spectral flux onset）检测算法实现。

敲击手势本身的分类模型（敲击/无敲击二分类，含一个小型 1D 卷积层）是基于以上信号在本机真实硬件上采集样本、独立训练得到的本地小型神经网络，非第三方预训练模型。

## 许可证

当前仓库沿用上游项目附带的 [LICENSE](LICENSE)。第三方依赖及参考项目保留各自许可证和版权声明。
