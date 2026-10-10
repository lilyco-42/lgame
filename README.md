<div align="center">

# lgame

**AI 控制实体的中间层。游戏是第一个落地场景，机器人是同一个抽象的下一个档位。**

`l3d` 3D 游戏 · `l2d` 2D 游戏 · 一个二进制同时是 CLI / TUI / Web / MCP

[![CI](https://github.com/lilyco-42/lgame/actions/workflows/ci.yml/badge.svg)](https://github.com/lilyco-42/lgame/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)

</div>

---

## 这是什么

一个把「**AI 想做什么**」和「**具体怎么动**」拆开的中间层。

```
       ┌────────────┐   观察 Observation    ┌────────────┐
       │   Driver   │ ────────────────────▶ │   Policy   │
       │  看 / 做    │                       │   想        │
       └────────────┘ ◀──────────────────── └────────────┘
              │             动作 Action
              ▼
         StepRecord ──▶ trace.jsonl ──▶ 下一轮提示词
```

四块地基，**只有一块与平台绑定**：

| 模块 | 职责 | 与平台绑定？ |
|---|---|---|
| `action` | 动作空间：`forward` / `strafe` / `yaw` / `pitch` + 语义动词 | 否 |
| `embodiment` | 躯体档位：有没有视线轴、连续量还是八方向 | 否 |
| `driver` | 看一帧 + 做一个动作 | **是**（屏幕档位 / 将来的机器人档位） |
| `policy` | 看 → 决定做什么 | 否（LLM / 离线脚本） |

加一个 `session` 把四者串成循环，`trace` 把每一步记成 JSONL。

## 为什么动作不是「按 W 键」

```
3D 第一人称（Minecraft）  forward=1 → 按住 W        + 鼠标相对位移
2D 平台跳跃（AIC）        forward=1 → 按住 W        （没有视线轴）
机器人底盘（将来）         forward=1 → 线速度 0.5m/s + 云台角速度
```

同一份 `Action`，三种落地方式，**策略层与提示词完全不用改**。
`Embodiment::robot()` 已经作为占位档位写进代码 —— 不是为了好看，
是为了让「这套抽象是不是真的不绑游戏」在编译期就能被检验。

## 快速开始

```bash
# 0) 自检：能看到哪些窗口？本地 LLM 端点通不通？
l3d doctor

# 1) 看内置目标（优先选你拥有源码的游戏）
l3d targets

# 2) 抓一帧，确认窗口匹配对了（只读，不会动任何键鼠）
l3d observe --target minecraft

# 3) 试跑一步：观察 → 让本地模型决策 → 只打印，不按键
l3d step --target minecraft --goal "走出这片树林"

# 4) 确认决策合理，再真的驱动
l3d run --target minecraft --goal "砍三棵树" --steps 20 --apply

# 5) 不启动 cc-switch 也能验证整条链路（离线脚本策略）
l3d run --target minecraft --goal "demo" --steps 6 --replay --apply
```

2D 游戏把 `l3d` 换成 `l2d` 即可：

```bash
l2d doctor
l2d run --target aic --goal "往右走并跳过缺口" --steps 30 --apply
```

## 内置目标

选型原则：**优先自有源码的游戏**（能加原生桥、能改键位、能确定性重放），
且必须覆盖两种躯体形态。

| id | 游戏 | 躯体 | 为什么选它 |
|---|---|---|---|
| `minecraft` | Minecraft Java 版 | 3D | 第一人称 3D 的基准靶子；生态里已有 Fabric 模板与服务器控制器 |
| `zombie-raid` | zombie-raid（自有 Godot 4.7） | 3D | 自有源码，将来可走 Godot 远程调试协议拿真实坐标 |
| `aic` | Alice in Cradle | 2D | 2D 动作平台基准；已有 BepInEx 工具链 |
| `stronghold` | 卫戍协议：盟约（自有浏览器 2D） | 2D | 自有源码，浏览器目标将来可走 CDP 直接调游戏内部 API |
| `tsukishita` | tsukishita（自有浏览器游戏） | 2D | 低风险实验场 |

不想用内置的？`--window "任意窗口标题的子串"` 直接指定。

## 工具（MCP / CLI 同一份）

| 工具 | 分级 | 作用 |
|---|---|---|
| `doctor` | T0 | 环境自检：可见窗口 / LLM 端点 / 内置目标 |
| `targets` | T0 | 内置目标与躯体档位 |
| `observe` | T0 | 抓一帧画面存 PNG |
| `act` | T1 | 执行一个动作（**默认试跑**） |
| `step` | T1 | 走一步：观察 → 决策 →（可选）执行 |
| `run` | T1 | 连续走多步，落 `trace.jsonl` |
| `think` | T1 | 用宿主 MCP 客户端的 LLM 决策（采样桥） |

给 Agent 用：

```bash
l3d --mcp        # MCP stdio 服务器，tools/list 一次返回全部工具
l3d --gui        # Web 控制台（SSE 进度）
l3d --tui        # TUI 命令选择页
l3d --schema     # 打印整张注册表 JSON 清单（全部工具 schema）
```

> **参数名是 kebab-case**：lilyco 会把字段名的 `_` 换成 `-`（`duration_ms` → `duration-ms`、
> `max_tokens` → `max-tokens`）。Agent 从 `tools/list` 拿到的是 kebab-case 名，照抄即可。
> 注意这与**动作对象**的字段名不同 —— LLM 输出的动作 JSON 用的是 `duration_ms`（见下方提示词）。

## 本地免费 API（cc-switch）

默认不需要任何配置。工具会读 `~/.claude/settings.json`：

```json
{ "env": {
    "ANTHROPIC_BASE_URL": "http://127.0.0.1:15721",
    "ANTHROPIC_AUTH_TOKEN": "PROXY_MANAGED",
    "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-5[1M]"
} }
```

`PROXY_MANAGED` 表示真实凭据由本地代理注入 —— 所以工具**不需要也不持有任何密钥**。

也可以用环境变量显式覆盖：

```bash
export LGAME_LLM_BASE_URL=http://127.0.0.1:15721
export LGAME_LLM_API_KEY=PROXY_MANAGED
export LGAME_LLM_MODEL=claude-sonnet-5
```

端点在听但没启动？`l3d doctor` 会直接告诉你「端口没有在听，请先启动 cc-switch」。

## 安全边界

**所有会注入键鼠的工具都有 `apply: bool`，缺省即试跑。**

这里有一个容易踩的坑：lilyco 的 `#[derive(App)]` 里 `default` **只写进 schema**
（供 CLI 填 `default_value`），而 MCP 的 `tools/call` 缺参时兜底是 `false`。
所以如果写成 `dry_run: bool`（声明默认 true），MCP 客户端省略该参数就会拿到
`false` —— **真的开始按键**。这是安全反转。

因此这里用**正向 opt-in**：`apply=true` 才动手，两种调用面的行为才一致。
同理，可选标量全部用 `Option<T>` + 代码内 `unwrap_or(默认值)`，
而不是依赖 `default`（否则 `max_tokens` 缺省会被兜底成 `0` 而不是 512）。

`Registry` 的安全策略两端都用 `Interactive`（放行 T1，拒绝 T3）。
与 `lfiles` 用 `DenyElevated` 不同 —— 文件工具的 MCP 面不该让 Agent 悄悄改文件，
而 lgame 的存在意义就是让 Agent 驱动输入。这个差异是有意的，写在 `registry.rs` 里。

⚠️ 还有一处**更隐蔽**的坑：`lilyco::run_registry_with` 会按后端**覆盖**你传入的策略
（MCP → `DenyElevated`）。用它的话，MCP 面上 `act`/`step`/`run`/`think` 会被全部拒掉。
所以两个二进制统一走 `lgame_tools::run_binary()`，内部用 `run_registry_with_policy`
（内层 + 外层都装 `Interactive` 门）。见 `registry.rs::run_binary` 的文档。

## 构建与测试

```bash
cargo test --workspace
cargo build --workspace --release
```

CI 在 `windows-latest` 与 `ubuntu-latest` 上跑测试与构建。
Linux 需要 xcap 的系统依赖（见 `.github/workflows/ci.yml`）：
`pkg-config libclang-dev libxcb1-dev libxcb-randr0-dev libxcb-shm0-dev
libxcb-xfixes0-dev libxrandr-dev libdbus-1-dev libegl-dev libgbm-dev libdrm-dev
libwayland-dev libpipewire-0.3-dev`。
（`libgbm-dev` 不能少 —— 缺它链接期会报 `unable to find library -lgbm`，
是 xcap 的 DMABUF 路径带来的。）

`lgame-core` 可以关掉平台相关的屏幕档位，在 Android / headless 上纯逻辑编译：

```bash
cargo build -p lgame-core --no-default-features
```

## 仓库结构

```
lgame/
├── lgame-core/      纯中间层（不依赖 lilyco —— 将来机器人可直接复用）
│   ├── action.rs        动作空间 + 物理扫描码表
│   ├── embodiment.rs    躯体档位（3d / 2d / robot）
│   ├── driver.rs        看 / 做（屏幕档位）
│   ├── capture.rs       截屏（xcap）
│   ├── input.rs         输入注入（enigo）
│   ├── policy.rs        决策（LLM / 离线脚本）
│   ├── ccswitch.rs      本地免费 API 发现
│   ├── session.rs       观察→决策→执行→记录 循环
│   ├── trace.rs         JSONL 轨迹
│   ├── targets.rs       内置目标表
│   └── observation.rs / util.rs / error.rs
├── lgame-tools/     lilyco 工具层（7 个 App + 注册表装配 + run_binary 入口）
├── l3d/             3D 游戏二进制（默认躯体 3d，main.rs 只有一行）
└── l2d/             2D 游戏二进制（默认躯体 2d，main.rs 只有一行）
```

## 依赖

- [`lilyco`](https://github.com/lilyco-42/lilyco) — 一个 struct 出 CLI/TUI/Web/MCP 四端
- [`xcap`](https://crates.io/crates/xcap) — 跨平台截屏
- [`enigo`](https://crates.io/crates/enigo) — 跨平台键鼠注入
- [`ureq`](https://crates.io/crates/ureq) — 同步 HTTP（调 LLM 端点）

`lilyco` 通过 git 依赖锁定到具体 rev（组织守则：git 依赖必须锁 rev/tag，不能锁 branch）。

## License

MIT
