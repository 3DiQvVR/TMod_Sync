# tModSync

tModSync 是一个面向 tModLoader 房主和队友的桌面启动器。房主分享本次联机的模组清单，队友同步启用状态与配置，再通过 Tailscale 或 ZeroTier 的虚拟 IP 进入游戏。

## 项目目录

```text
tModSync/
├── schemas/room-profile.schema.json    Room Profile v1 的 JSON Schema
├── src/                               React、TypeScript、Tailwind 与 Lucide 工作台
├── public/                            本地像素画与品牌图标
├── src-tauri/
│   ├── src/main.rs                    Tauri IPC、文件选择、剪贴板、启动预检
│   ├── src/paths.rs                   路径探测与 config.json
│   ├── src/profile.rs                 TMOD 文件头、SHA-256、清单与差异
│   ├── src/sync.rs                    文件备份、同步与中断恢复
│   ├── src/sync_mods.rs               校验本地模组、检索工坊并准备复制
│   ├── src/network.rs                 组网 CLI 探测、连接与 IP
│   ├── src/launch.rs                  启动游戏与交互式独立服务端
│   ├── src/server.rs                  本地世界、后台进程、开服配置和日志尾部
│   ├── Cargo.toml / Cargo.lock         Rust 依赖
│   └── tauri.conf.json                 桌面窗口、资源与打包设置
└── package.json / package-lock.json    前端依赖与运行脚本
```

## 房间清单

默认导出文件名为 `room-profile.json`。下面这份文档符合 v1 结构，用于说明字段。示例名称、版本和散列用于演示，实际联机时使用房主导出的文件。

```json
{
  "schemaVersion": 1,
  "roomName": "周末专家档",
  "mods": [
    {
      "name": "ExampleMod",
      "version": "1.2.0",
      "sha256": "af680978ab59d28505dc04fc052943dfa28e697938fab8ce97d45cdbabe99015",
      "workshopId": "1234567890"
    }
  ],
  "configs": {
    "ExampleMod_ServerSettings.json": {
      "difficulty": 2
    }
  }
}
```

| 字段 | 含义 |
| --- | --- |
| `schemaVersion` | 本版固定为整数 1。 |
| `roomName` | 房主填写的房间名称。 |
| `mods[].name`、`version` | 来自实际 `.tmod` 文件头的内部名称与模组版本。数组同时定义 `enabled.json` 的目标集合。 |
| `mods[].sha256` | 对完整文件流计算的 SHA-256，使用 64 位小写十六进制。版本字符串相同而文件不同，也会阻止启动。 |
| `mods[].workshopId` | 可选的 Steam 项目编号。作为字符串传递，以保留完整精度。 |
| `configs` | `ModConfigs` 顶层的 JSON 文件名与内容。同步以房间清单为准，多余的顶层 JSON 会先备份再移出。 |

完整定义见 [JSON Schema](schemas/room-profile.schema.json)。清单以原始 JSON 文本跨越桌面桥接层，配置中的大整数不会经过 JavaScript 数字转换。

## 从源码运行

需要 Node.js 22 或更新版本、npm、稳定版 Rust 和目标平台的 Tauri v2 开发依赖。Windows 需要 MSVC C++ 构建工具、Windows SDK 和 WebView2。macOS 需要 Xcode 命令行工具，Linux 需要 WebKitGTK 4.1 等桌面开发库。[Tauri 平台准备](https://v2.tauri.app/zh-cn/start/prerequisites/)

在项目根目录执行：

```sh
npm ci
npm run tauri dev
```

编译、测试与打包：

```sh
npm run build
cargo test --locked --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

`npm run dev` 只提供前端开发服务器。文件选择、组网与游戏启动通过 `npm run tauri dev` 打开的桌面窗口操作。正式可执行文件生成于 `src-tauri/target/release/`，安装包位于其 `bundle/` 子目录。

## 第一次联机

启动时会检测 Tailscale 和 ZeroTier。首次使用时，如果只检测到其中一种，启动器会优先选用它；已有设置按原选择使用。两种都未检测到时，页面顶部提供 [Tailscale 官网下载](https://tailscale.com/download) 和 [ZeroTier 官网下载](https://www.zerotier.com/download/)。安装后可点击「重新检测」，后台也会定期刷新。

在引导卡上点击「连接 Tailscale」，或输入房主提供的 16 位 ZeroTier 网络 ID 后点击「加入网络」。需要登录或授权时，按本机客户端提示完成。连接成功后房主可直接复制虚拟 IP 和端口。地址优先来自组网服务，缺少地址时会检查相应的活动虚拟网卡。仅发现网卡地址时会提示「服务连接待确认」，启动预检继续等待服务确认。

1. 两人各自从 Steam 安装并运行一次 tModLoader，然后关闭游戏。在启动器的路径设置中确认游戏数据根目录、游戏安装目录与创意工坊目录。Windows 支持重定向的“文档”目录以及 Steam 的多磁盘游戏库。数据根目录应包含 `Mods`，相邻的 `ModConfigs` 可以在首次同步时创建。
2. 两人选择相同的组网服务。Tailscale 需要登录同一 tailnet 或具备共享设备访问权限。ZeroTier 需要加入同一网络，私有网络还需由网络管理员授权成员。状态卡显示本机虚拟 IP。节点在线后，游戏仍需要房主开始托管并允许对应端口的入站连接。[Tailscale CLI](https://tailscale.com/docs/reference/tailscale-cli) · [ZeroTier CLI](https://docs.zerotier.com/cli/)
3. 房主选择房主模式，读取当前模组，导出清单给队友。实际名称与版本来自 TMOD 文件头，校验值在本机计算。若游戏同时保留多个工坊分支，可在路径设置中指定本次使用的 tModLoader 月份。[tModLoader 文件格式实现](https://github.com/tModLoader/tModLoader/blob/stable/patches/tModLoader/Terraria/ModLoader/Core/TmodFile.cs) · [工坊模组选择实现](https://github.com/tModLoader/tModLoader/blob/stable/patches/tModLoader/Terraria/ModLoader/Core/ModOrganizer.cs)
4. 队友导入清单，点击「同步模组与配置」。启动器核对本地文件的内部名称、版本与完整 SHA-256，并从已下载的工坊项目及其版本子目录复制精确匹配的文件。工坊仍缺少对应文件时，可打开该项目的订阅页；没有工坊 ID 时，按钮会按名称搜索 tModLoader 创意工坊。下载完成后再次同步。工坊已替换的旧版本可向房主获取。
5. 房主进入「独立服务端」，从当前游戏数据目录的 `Worlds/*.wld` 选择世界，设置人数（1–255，默认 8）、可选密码和端口，点击「一键开服」。已有房间清单时按清单预检，否则读取房主当前环境。页面每 750 毫秒刷新控制台最后 10 行；游戏输出开始监听后，即可复制虚拟 IP 和实际开服端口给队友。队友粘贴地址后启动客户端进入世界。
6. 每次启动时 Rust 都会重新读取磁盘，检查启用集合、模组版本、SHA-256 和配置，再查询当前虚拟网络状态。任一项失败，界面显示原因，后端中止启动。

## 设置与备份

每次后台开服都会在启动器的应用配置目录中新建独立的会话目录，写入 UTF-8 的 `serverconfig.txt`。配置中的密码按游戏要求保存为明文，进程正常退出并由启动器检测到后会删除该文件。服务器面板的「保存并停服」会发送游戏控制台命令 `exit`。正常关闭启动器时也会请求保存并等待服务端退出。

Windows 原版批处理使用 `start` 分离控制台。为接收输出并发送停服命令，启动器在会话目录运行官方 `start-tModLoaderServer.bat` 的副本，用随游戏安装的 BusyBox 在后台执行官方启动脚本。安装目录中的脚本与默认配置保持原样。Linux 和 macOS 使用对应的 Bash 启动脚本。[官方服务端配置示例](https://github.com/tModLoader/tModLoader/blob/stable/patches/tModLoader/Terraria/release_extras/serverconfig.txt)

启动器的 `config.json` 位于系统应用配置目录中的 `io.tmodsync.launcher` 文件夹。Windows 使用 `%APPDATA%`，macOS 使用 `~/Library/Application Support`，Linux 使用 `$XDG_CONFIG_HOME`（默认 `~/.config`）。保存路径不会编辑 Terraria 的 `config.json`。

同步前请关闭使用同一数据目录的游戏和服务端。启动器会备份待替换的模组与配置，校验复制结果，最后按照房主清单的顺序原子写入 `Mods/enabled.json`。即使仍需下载部分模组，启用列表也会写为完整房间清单；队友的启动预检会持续拦截至所有指定文件就位。无关模组保留在磁盘上。备份位于数据根目录的 `.tmodsync-backups`，未完成的同步会在下次读取清单或同步时自动回滚。

