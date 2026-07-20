# UsageMonitor · AI 使用量监控悬浮窗

一款 Windows 桌面小工具，实时统计 **Claude Code** 与 **Codex** 的 token 使用量并换算为美元花费。以毛玻璃半透明悬浮窗常驻桌面，悬停即可查看当日明细，点击展开历史趋势、项目排行与设置面板。

> 数据完全来自本地会话日志（`~/.claude` / `~/.codex`），不依赖云端、不上传任何数据。

## 功能特性

- **桌面悬浮窗**：150×42 胶囊小组件，置顶、可拖动、免任务栏驻留，右键菜单（查看详情 / 设置 / 退出）
- **悬停速览**：鼠标悬停弹出当日摘要窗（花费、token、各工具明细、当前供应商余额）
- **详情面板**：
  - 今日概览：花费大数字、输入/输出/缓存 token 分项、工具占比、7 天迷你趋势图
  - 历史记录：按天聚合的花费趋势与会话明细（今日 / 本周 / 本月 / 全部）
  - 按项目：各项目累计花费、会话数、Claude/Codex 占比排行
  - 按模型：各模型 token 与花费分布
- **价格体系**：
  - 内置 Anthropic / OpenAI 官方默认单价
  - 自动从 [cc-switch](https://github.com/farion1231/cc-switch) 的 `model_pricing` 表同步 160+ 模型单价（含 glm、gpt 系列）
  - 设置页可手动增改单价，改价后一键全量重算历史花费
  - 未匹配单价的模型显示 `—` 并提示补价，token 照常统计
- **供应商余额**：读取 cc-switch 当前供应商配置，展示余额 / 套餐用量（支持 balance 与 token_plan 两种模板）
- **任务栏集成**：可在任务栏实时显示今日花费
- **增量索引**：首次启动后台全量索引历史日志（带进度提示），之后按可配置间隔（默认 30s）增量扫描当日文件，文件轮转/截断自动重扫
- **主题切换**：多窗口联动的明暗主题

## 技术栈

| 层 | 技术 |
|------|------|
| 桌面框架 | Tauri 2 |
| 前端 | React 19 + TypeScript + Tailwind CSS + Framer Motion |
| 后端 | Rust（rusqlite / tokio / chrono / windows-rs） |
| 存储 | SQLite（WAL 模式，位于用户数据目录） |
| 数据源 | `~/.claude/projects/**/*.jsonl`、`~/.codex/sessions/**/*.jsonl` |

## 架构

```
┌─────────────────────────────────────────────┐
│  前端 (React + TS + Tailwind)                │
│  悬浮窗 + 悬停速览 + 详情/历史/项目/设置面板   │
└───────────────┬─────────────────────────────┘
                │ Tauri command / event
┌───────────────┴─────────────────────────────┐
│  query 层  ── 今日/历史/项目/模型聚合查询      │
│  indexer 层 ── 全量索引 + 定时增量（file 偏移） │
│  parsers 层 ── claude_parser / codex_parser  │
│              （统一 UsageParser trait）       │
│  ccswitch  ── 价格同步 + 供应商余额查询        │
│  taskbar / windows_topmost / widget_mouse   │
│              （Win32 原生窗口集成）           │
└─────────────────────────────────────────────┘
                ↑ 读取
        ~/.claude/projects/**/*.jsonl
        ~/.codex/sessions/**/*.jsonl
```

- **claude_parser**：解析 `type:"assistant"` 记录的 `message.usage`，项目路径由编码目录名反解
- **codex_parser**：解析 `session_meta`（cwd/model）与 `token_count` 累计值，做差分得到增量，reasoning 计入 output
- **容错**：单行损坏跳过不中断；model 缺失记 `unknown`；目录不存在优雅降级

## 目录结构

```
usage-monitoring/
├── src/                          # 前端 React
│   ├── components/
│   │   ├── widgets/CardWidget.tsx   # 悬浮窗组件
│   │   ├── HoverDetailWindow.tsx    # 悬停速览窗
│   │   ├── DetailPanel.tsx          # 详情面板
│   │   ├── HistoryPage.tsx          # 历史记录
│   │   ├── ProjectsPage.tsx         # 按项目
│   │   └── SettingsPage.tsx         # 设置
│   ├── hooks/                       # Tauri command 封装
│   └── lib/                         # api / 拖拽 / 格式化
├── src-tauri/                    # Rust 后端
│   └── src/
│       ├── parsers/              # claude.rs / codex.rs + trait
│       ├── indexer.rs            # 全量 + 增量索引
│       ├── query.rs              # 查询 command
│       ├── db.rs                 # SQLite 连接 / 迁移 / 价格种子
│       ├── ccswitch.rs           # cc-switch 价格同步与余额
│       ├── taskbar.rs            # 任务栏实时花费
│       ├── windows_topmost.rs    # 悬浮窗置顶保活
│       ├── widget_mouse.rs       # 原生鼠标钩子（悬停/右键）
│       └── window_drag.rs        # 原生拖拽与窗口摆放
└── docs/                         # 设计文档
```

## 开发

### 环境要求

- Node.js 18+
- Rust stable（含 MSVC 工具链）
- Windows 10/11（依赖系统 WebView2）

### 常用命令

```bash
npm install          # 安装前端依赖
npm run tauri dev    # 开发模式（前端热更新 + Rust 自动重编译）
npm run tauri build  # 构建发布版（NSIS 安装包，产物在 src-tauri/target/release/bundle/）
```

### 调试

后端使用 `tracing` 输出日志，可通过环境变量控制级别：

```bash
RUST_LOG=usage_monitoring_lib=debug npm run tauri dev
```

## 平台说明

当前仅针对 **Windows** 优化（毛玻璃、置顶保活、原生鼠标钩子、任务栏集成均使用 Win32 API）。Tauri 本身跨平台，但 macOS/Linux 适配不在首版目标内。

## 隐私

所有统计均在本地完成：读取本机 Claude Code / Codex 会话日志 → 写入本地 SQLite。仅在查询供应商余额时按 cc-switch 中配置的 API 端点发起网络请求。
