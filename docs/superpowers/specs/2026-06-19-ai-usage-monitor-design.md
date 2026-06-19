# AI 使用量监控工具 · 设计文档

- **日期**：2026-06-19
- **状态**：待实现
- **作者**： brainstorming 协作产出

## 1. 目标

一款 Windows 桌面工具，统计 **Claude Code** 和 **Codex** 的当日 token 使用量并换算为美元，支持悬浮窗（毛玻璃半透明，macOS/iOS 控件中心风格）与任务栏驻留两种呈现，点击可查看历史使用量与每个项目的用量/花费明细。

最终交付为**单个可执行 exe**（免安装，依赖系统自带 WebView2）。

## 2. 关键决策汇总

| 维度 | 决策 |
|------|------|
| 桌面框架 | Tauri 2 |
| 前端 | React + TypeScript + Tailwind CSS |
| 数据源 | 本地 `~/.claude/projects/**/*.jsonl` 与 `~/.codex/sessions/**/*.jsonl` |
| 存储 | SQLite（WAL 模式） |
| 悬浮窗形态 | 卡片 / 胶囊 / 仪表盘 / 隐藏，设置里分段切换 |
| 任务栏形态 | 托盘图标 / 实时数字 / 关闭，与悬浮窗独立配置 |
| 价格 | 内置 Anthropic/OpenAI 官方默认价；其他模型（如 glm-5.1）设置里自填 |
| 未知模型花费 | 显示 `—` 并提示补价，token 照常统计 |
| 数据更新 | 定时扫描，间隔可设（默认 30s）；历史数据首次全量索引后缓存，仅定时刷新"今日" |
| 历史范围 | 全部历史；首次后台全量索引（带进度提示），之后增量维护 |
| 架构 | 分层 + trait 化数据源（parsers → indexer → query） |
| 打包 | 单 exe（NSIS 单文件或主 exe + 系统 WebView2） |

## 3. 架构

```
┌─────────────────────────────────────────────┐
│  前端 (React + TS + Tailwind)                │
│  悬浮窗(卡片/胶囊/仪表盘) + 详情面板 +        │
│  历史/项目页 + 设置页                         │
└───────────────┬─────────────────────────────┘
                │ Tauri command / event
┌───────────────┴─────────────────────────────┐
│  query 层 ── 供前端查询今日/历史/项目聚合      │
│       ↑                                      │
│  indexer 层 ── 解析结果入 SQLite + 增量维护   │
│       ↑                                      │
│  parsers 层 ── claude_parser / codex_parser  │
│  (实现统一 trait: 扫描目录→产出 UsageRecord)  │
└─────────────────────────────────────────────┘
                ↑ 读取
        ~/.claude/projects/**/*.jsonl
        ~/.codex/sessions/**/*.jsonl
```

Tauri 后端用 Rust，前端用 React。前端通过 Tauri command 拉数据、通过 event 接收后台进度通知。窗口使用 Windows 原生毛玻璃效果（mica/acrylic，经 Tauri 窗口配置 + 前端半透明背景叠加 `backdrop-filter`）。

## 4. 数据模型（SQLite）

### 4.1 `usage_records`

| 字段 | 类型 | 说明 |
|------|------|------|
| id | INTEGER PK | 自增 |
| date | TEXT | YYYY-MM-DD（按记录时间戳归日） |
| tool | TEXT | `claude` \| `codex` |
| project | TEXT NULL | 项目路径（Claude 为编码后的目录反解，Codex 取 cwd） |
| model | TEXT | 模型名，缺失记 `unknown` |
| session_id | TEXT | 会话 ID |
| input_tok | INTEGER | 输入 token |
| output_tok | INTEGER | 输出 token |
| cache_tok | INTEGER | 缓存 token |
| cost_usd | REAL | 换算花费，未知模型为 0 |
| priced | INTEGER | 0/1，是否匹配到单价 |
| timestamp | INTEGER | 记录时间戳（秒） |
| source_file | TEXT | 来源 jsonl 文件路径 |

索引：`(date)`、`(tool)`、`(project)`、`(source_file, session_id, timestamp)` 唯一约束去重。

### 4.2 `pricing`

| 字段 | 类型 |
|------|------|
| model | TEXT PK |
| in_per_mtok | REAL（每百万输入 token 美元） |
| out_per_mtok | REAL（每百万输出 token 美元） |
| cache_per_mtok | REAL（每百万缓存 token 美元） |
| builtin | INTEGER（0/1 是否内置默认） |

内置默认值（首次建库写入，可改）：
- `claude-sonnet-4`：in 3.0 / out 15.0 / cache 0.30
- `claude-opus-4`：in 15.0 / out 75.0 / cache 1.50
- `claude-haiku-*`：in 0.80 / out 4.0 / cache 0.08
- `gpt-5`：in 1.25 / out 10.0 / cache 0.125
- `gpt-5-mini`：in 0.25 / out 2.0 / cache 0.025
（具体默认值在实现时以官方当时公开价为准，留 TODO 校准。）

### 4.3 `file_state`（增量索引偏移）

| 字段 | 类型 |
|------|------|
| source_file | TEXT PK |
| tool | TEXT |
| file_offset | INTEGER（上次读到的字节位置） |
| last_seen | INTEGER |

文件被截断/轮转（当前大小 < 已存 offset）时，偏移重置为 0 重扫。

### 4.4 `settings`（KV）

| key | 值 |
|-----|-----|
| widget_mode | card \| pill \| gauge \| hidden |
| taskbar_mode | tray \| live_number \| off |
| scan_interval_sec | 整数，默认 30 |
| indexed | 0/1 |
| db_version | 整数（迁移用） |

## 5. parsers 层

统一 trait：

```rust
pub struct UsageRecord {
    pub date: String,
    pub tool: Tool,            // Claude | Codex
    pub project: Option<String>,
    pub model: String,
    pub session_id: String,
    pub input_tok: u64,
    pub output_tok: u64,
    pub cache_tok: u64,
    pub timestamp: i64,
    pub source_file: PathBuf,
}

pub trait UsageParser {
    fn source_root(&self) -> PathBuf;
    fn discover_files(&self) -> Vec<PathBuf>;
    fn parse_file(&self, path: &Path, start_offset: u64) -> (Vec<UsageRecord>, u64);
}
```

### 5.1 claude_parser

- 根目录 `~/.claude/projects/<编码项目路径>/`
- 项目名反编码：Claude 把原始 cwd 路径中的 `:` 和路径分隔符（`\`/`/`）统一替换为 `-`。反编码规则：把首段的双连字符 `--` 还原为 `:\`（盘符，如 `E--` → `E:\`），其余单个 `-` 还原为 `\`。示例：`E--Idea-Project-code-test` → `E:\Idea\Project\code-test`。（实现时以实际样本验证，若路径本身含连字符则可能产生歧义，按最常见 Windows 路径规则处理即可。）
- 逐行解析 JSONL，只取 `type:"assistant"` 的记录
- 读取 `message.usage`：input_tokens / output_tokens / cache_creation_input_tokens / cache_read_input_tokens
- `cache_tok = cache_creation + cache_read`
- `model = message.model`
- 按记录时间戳归日

### 5.2 codex_parser

- 根目录 `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`
- `session_meta` 提取 `cwd`（项目路径）和 `model`
- `token_count` 事件的 `total_token_usage`：input / cached / output / reasoning tokens
- **累计差分**：token_count 是会话累计值，需用本次 - 上次得到增量；reasoning 计入 output
- 项目取 `cwd`

### 5.3 容错

- 单行解析失败：跳过该行，不中断
- model 字段缺失：记 `"unknown"`
- 文件读取错误：记录日志跳过

## 6. indexer 层

- **首次索引**：`settings.indexed = 0` 时后台线程全量扫描所有历史文件，批量写入。前端通过 event 接收进度（IndexedFiles/TotalFiles），详情面板显示"索引中 N/M"。完成后置 `indexed=1`。
- **定时增量**：tokio 定时任务，间隔取自 settings（默认 30s）。每轮：
  1. 发现"今日"相关文件（Claude 按项目目录、Codex 按 YYYY/MM/DD）
  2. 对每个文件从 `file_state.file_offset` 续读，解析新增行
  3. UPSERT 进 `usage_records`
- **防抖**：上一轮未完成则跳过本轮
- **并发**：SQLite 单写连接（Mutex），读连接可并发；WAL 模式

## 7. query 层

| Command | 输入 | 返回 |
|---------|------|------|
| `get_today_summary` | — | 今日总花费、总 token、输入/输出/缓存分项、Claude/Codex 明细 |
| `get_range_summary` | range | 该范围汇总 + 工具明细 |
| `get_history` | range, tool? | 按天聚合（日期→花费/token/会话数）供趋势图 |
| `get_daily_sessions` | date | 指定天的会话级明细 |
| `get_projects` | sort | 项目排行：累计花费、会话数、Claude/Codex 占比 |
| `get_project_sessions` | project | 该项目会话历史 |
| `recompute_cost` | — | pricing 改后全量重算 cost_usd |
| `get_pricing` / `set_pricing` | — | 读取/更新单价表 |
| `get_settings` / `set_settings` | — | 读写设置 |

花费计算：`cost = input/1e6*in + output/1e6*out + cache/1e6*cache`。无匹配 model → `priced=0`，UI 显示 `—`。

聚合结果按 (range/tool) 内存缓存，定时扫描后只失效"今日"相关缓存。

## 8. UI 设计

### 8.1 悬浮窗（三态 + 隐藏）

- **卡片**：竖向圆角卡片（~168px），显示日期、今日花费大数字、Claude/Codex 各一行 token 明细（带色点）
- **胶囊**：横向胶囊（~300px），花费 / Claude / Codex 三段分隔排列，适合贴屏幕顶/底边
- **仪表盘**：圆形（~120px）圆环进度，中心显示花费，点击展开详情
- **隐藏**：不显示悬浮窗
- 共性：可拖动、置顶、毛玻璃半透明、右键菜单（设置/退出）
- 点击（除隐藏外）展开详情面板

### 8.2 详情面板（今日概览，默认）

- 头部：日期 + 今日花费大数字 + token 总数
- 三汇总芯片：输入 / 输出 / 缓存 token
- 时间范围 Tab：今日 / 本周 / 本月 / 全部
- 工具明细：Claude Code、Codex 各一行，带进度条和占比
- 7 天迷你柱状趋势图（今日高亮绿）
- 底部入口：历史记录 / 按项目

### 8.3 历史记录页（二级）

- 折线趋势图（每日花费）+ 统计三连（总计 / 日均 / 峰值）
- 时间范围 Tab：7天 / 30天 / 全部
- 按天分组展开，每天显示项目、工具、会话数、token、花费明细
- 可下滑加载更多

### 8.4 按项目页（二级）

- 按花费/Token 排序
- 每行：项目名、会话数、累计 token、Claude/Codex 占比分段进度条、占比百分比
- 点项目展开该项目会话历史

### 8.5 任务栏（独立于悬浮窗）

- **托盘图标**：常驻系统托盘，hover 显示摘要，点击展开
- **实时数字**：任务栏按钮直接显示今日花费（如 `$2.47`）
- **关闭**：不驻留任务栏
- 所有模式下右键托盘均可访问设置/退出

### 8.6 设置页

- 悬浮窗模式分段控件（卡片/胶囊/仪表盘/隐藏）
- 任务栏模式分段控件（托盘/实时数字/关闭）
- 扫描间隔（秒数输入或滑块）
- 价格表编辑器：列出所有 model，每行可编辑 in/out/cache 单价；支持新增自定义模型
- 数据：手动重算花费、重新全量索引、清空数据

### 8.7 视觉规范

- 毛玻璃：`backdrop-filter: blur(24-28px) saturate(180%)`，背景 `rgba(255,255,255,0.12)`，边框 `rgba(255,255,255,0.18)`，内阴影高光
- 圆角：卡片 20-24px，芯片 14px，按钮 10-11px
- 配色：Claude = `#ff8c42`（橙），Codex = `#34c759`（绿），强调蓝 `#5ac8fa`
- 字体：系统默认无衬线，字重对比鲜明（大数字 700，标签 600 大写）
- 动效：Framer Motion 做展开/切换过渡，符合 iOS 控件中心流畅感

## 9. 错误处理

- 数据源目录不存在（用户未装 Claude/Codex）：优雅降级，悬浮窗显示 `—`，设置里提示未检测到数据源
- 首次索引失败：记录错误日志，UI 提示"索引失败，点击重试"，不阻塞悬浮窗显示今日数据
- JSONL 部分损坏：逐行容错跳过
- 数据库锁/写失败：重试 3 次后记录日志，查询返回上次缓存
- 定时扫描异常：捕获 panic 不让定时器挂掉

## 10. 打包

- Tauri 2 配置 `bundle.targets = ["nsis"]`
- 产出单文件 NSIS 安装包或免安装主 exe（依赖系统自带 WebView2，Win10/11 默认具备）
- 应用图标、版本号、应用名写入 `tauri.conf.json`
- 构建命令：`npm run tauri build`

## 11. 项目结构

```
usage-monitoring/
├── src/                          # 前端 React
│   ├── components/
│   │   ├── widgets/              # Card / Pill / Gauge 悬浮窗
│   │   ├── DetailPanel.tsx       # 详情面板
│   │   ├── HistoryPage.tsx       # 历史记录
│   │   ├── ProjectsPage.tsx      # 按项目
│   │   └── SettingsPage.tsx
│   ├── hooks/                    # tauri command 封装
│   ├── lib/                      # api 封装、格式化
│   ├── App.tsx
│   └── main.tsx
├── src-tauri/                    # Rust 后端
│   ├── src/
│   │   ├── parsers/
│   │   │   ├── mod.rs            # UsageParser trait
│   │   │   ├── claude.rs
│   │   │   └── codex.rs
│   │   ├── indexer.rs            # 索引 + 增量
│   │   ├── query.rs              # 查询 command
│   │   ├── db.rs                 # SQLite 连接 + 迁移
│   │   ├── pricing.rs            # 价格表 + 花费计算
│   │   ├── models.rs             # 数据结构
│   │   └── main.rs               # Tauri 入口
│   ├── Cargo.toml
│   └── tauri.conf.json
├── docs/
└── package.json
```

## 12. 范围外（YAGNI）

- 不做云端同步 / 账号登录
- 不做实时文件监听（watch），仅定时扫描
- 不做导出 CSV/图表截图（首版）
- 不做多语言（首版中文）
- 不做 macOS/Linux 原生适配（Windows 优先，Tauri 跨平台能力保留但非目标）
- 不做预算告警/通知（首版）

## 13. TODO / 待实现时确认

- 内置默认单价的准确数值（以实现时各官方公开价为准校准）
- glm-5.1 等自定义模型单价由用户首次启动后在设置里填写
