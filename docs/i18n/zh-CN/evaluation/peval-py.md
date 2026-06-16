# peval-py 轻量轨迹报告

语言：简体中文 | [English](../../../evaluation/peval-py.md)

`peval-py` 是 peval 的轻量 Python 版本，用于查看已保留的 agent 轨迹。
它读取 JSONL session 或 adapter 拥有的 SQLite database，并输出派生的 ATIF
轨迹或静态 peval 风格报告。它可以为 `serve` 初始化本地 peval workspace，但不会
运行 agent 或给任务打分。

安装、源码运行和本地二进制打包见
[tools/peval-py/README.zh-CN.md](../../../../tools/peval-py/README.zh-CN.md)。

## 转换 JSONL Session

需要原始 ATIF JSON 轨迹时，使用 `export trajectory`。`tr` 是
`trajectory` 的短写：

```bash
peval-py export tr -p session.jsonl -o
python -m json.tool trajectory-psychevo-<session>.json >/dev/null
```

JSONL 输入是一行一个 JSON 对象。每行可以是直接的 message 对象，也可以是包含
`message`、`usage`、`metadata`、`accounting` 和 `session_seq` 的 wrapper。

`-p/--path` 也接受导出的 ATIF JSON trajectory。ATIF JSON 输入不需要
`-a/--adapter`；它会作为 passthrough source 处理，并在 report metadata 中显示为
adapter `atif`：

```bash
peval-py view tr -p trajectory-opencode-<session>.json -o
```

来源不是默认 Psychevo adapter 时，用 `-a` 选择 adapter：

```bash
peval-py export tr -a opencode -p session.jsonl -o
peval-py export tr -a hermes -p session.jsonl -o
```

## 自定义 Agent Adapter

自定义 agent 如果有自己的 transcript 格式，可以通过 Python package entry point
接入 `peval-py`。这种方式不需要修改 `peval-py` 的核心 adapter 列表。

在 adapter 包的 `pyproject.toml` 中注册 `peval_py.adapters` entry point。
entry point 名称就是用户传给 `--adapter` 的 adapter id：

```toml
[project.entry-points."peval_py.adapters"]
custom = "custom_peval_adapter:CustomAdapter"
```

adapter 可以实现 `convert(records, config)` 或 `convert_path(path, config)`。
如果源文件能走默认 JSONL 或 SQLite `messages` loader，实现 `convert`。如果
adapter 需要自己解析输入文件，实现 `convert_path`。

```python
from peval_py.adapters.base import ConversionResult, StepMeta


class CustomAdapter:
    agent_id = "custom"

    def convert_path(self, path, config):
        return ConversionResult(
            trajectory={
                "schema_version": "ATIF-v1.7",
                "trajectory_id": "custom:t001",
                "agent": {
                    "name": config.agent_name or "custom",
                    "version": config.agent_version,
                },
                "steps": [
                    {
                        "step_id": 1,
                        "source": "user",
                        "message": "converted custom transcript",
                    }
                ],
                "final_metrics": {
                    "total_steps": 1,
                    "total_turns": 1,
                    "total_tool_calls": 0,
                    "total_tool_errors": 0,
                },
            },
            steps_meta=[StepMeta(step_id=1, source="user")],
            warnings=[],
            total_events=1,
            unmapped_events=0,
            started_at_ms=None,
            finished_at_ms=None,
        )
```

adapter 私有参数只走 TOML，不新增 CLI flags。`peval-py` 会把每个 effective
adapter 的配置表原样传给 `config.adapter_options`：

```toml
[defaults]
adapter = "custom"

[adapters.custom]
input_mode = "transcript"
```

adapter 表还可以设置 `default_db_path`。这是 peval-py 保留字段，不会传给
adapter 代码。`~` 会展开，相对路径按定义该值的 TOML 文件所在目录解析：

```toml
[adapters.psychevo]
default_db_path = ".psychevo/state.db"
```

运行方式和内置 adapter 相同：

```bash
peval-py view tr -c custom.toml -p custom-session.log -o
peval-py export tr -a custom -p custom-session.log -o
```

当输入没有显式 `-a`、`pN=`、`dN=` 或 manifest adapter 时，peval-py 可以从路径
推断 adapter。adapter id 必须作为完整 path component 或文件名 token 出现，因此
`.hermes/` 和 `.psychevo/` 下的路径会分别推断为 `hermes` 和 `psychevo`。如果同一
路径匹配多个 adapter，会失败并提示使用 `-a`。

如果自定义 adapter 只实现了 `convert_path`，请配合 `-p/--path` 使用。
SQLite `-d/--db` 输入可以由 adapter 实现 `convert_db(path, session_id, config)`
来接管数据库解析。没有 `convert_db` 的 adapter 仍可通过通用配置的 `messages`
表结构加载 records，再调用 `convert(records, config)`。

## 从 OpenCode DB 生成报告

`opencode` adapter 可以直接读取当前 OpenCode SQLite 持久化格式。通过 `--db`
传入 OpenCode database 路径。如果省略 `--session-id`，adapter 会选择最近更新的
session：

```bash
peval-py view tr \
  -a opencode \
  -d ~/.local/share/opencode/opencode.db \
  -o
```

需要指定 session 时：

```bash
peval-py view tr \
  -a opencode \
  -d ~/.local/share/opencode/opencode.db \
  -s <session-id> \
  -o
```

也可以先列出 session 再选择：

```bash
peval-py view tr -a opencode -d ~/.local/share/opencode/opencode.db --list
peval-py view tr -a opencode -d ~/.local/share/opencode/opencode.db -s #2 -o
peval-py view tr -a opencode -d ~/.local/share/opencode/opencode.db -li -o
```

对于包含 `event` 表的当前 OpenCode database，peval-py 会使用 event stream
从第一次 `running` start 到最终 `completed` 或 `error` end 还原工具执行时长。
模型 timing 会显示为 OpenCode assistant/tool 边界估算，不等同于 provider API
latency。没有匹配 event 的旧 database 会保留现有 part timestamp fallback。

## 从 Hermes DB 生成报告

`hermes` adapter 可以直接读取当前 Hermes SQLite 持久化格式。通过 `--db`
传入 Hermes database 路径。如果省略 `--session-id`，adapter 会选择最近活跃的
session。存在 `sessions.system_prompt` 时，报告会把它作为第一条 system step。

Hermes DB message timestamp 会被视为持久化/排序时间。报告仍会保留这些
timestamp 推导出的 wall duration，但 active model/tool duration 会保持 unknown，
除非 Hermes 记录里带有显式 elapsed/start/end timing metadata。对于当前 Hermes
DB 输入，peval-py 还会检查同级 `logs/agent.log`，并在 API/tool timing 与 DB
transcript 严格匹配时把它作为显式 model/tool duration；日志缺失或不匹配时仍保持
active timing unknown。

```bash
peval-py view tr \
  -a hermes \
  -d ~/.hermes/state.db \
  -o
```

需要指定 session 时：

```bash
peval-py view tr \
  -a hermes \
  -d ~/.hermes/state.db \
  -s <session-id> \
  -o
```

使用 `--list`/`-l` 可以打印 session 序号、id 和名称。使用 `-s #N` 按列表序号
选择，或使用 `--list-interactive`/`-li` 输入 `1,3-4`、`all` 等选择。

## 从 Psychevo State DB 生成报告

`view trajectory` 用于生成 peval 兼容 JSON 或离线 HTML 报告。这里同样可以用
`tr`。输出后缀会决定格式，所以通常不需要指定 `-f`。省略 `--session-id` 时，
adapter 会从 Psychevo `sessions` 表中选择最近更新的 session：

```bash
peval-py view tr \
  -d ~/.psychevo/state.db \
  -o
```

生成 JSON：

```bash
peval-py view tr \
  -d ~/.psychevo/state.db \
  -f json \
  -o

python -m json.tool report-psychevo-<session-id>.json >/dev/null
```

需要指定 session 时：

```bash
peval-py view tr \
  -d ~/.psychevo/state.db \
  -s <session-id> \
  -o
```

`peval-py view tr -d ~/.psychevo/state.db --list` 会打印 `#`、`session_id` 和
`name`。`-s 3` 会先尝试真实 session id `3`；如果不存在，才选择列表序号 3。
`-s #3` 永远表示序号 3。

Psychevo DB reader 会先选定 session，再读取该 session 的 `messages` 行，并把
选中的 session id 保留在轨迹和报告元数据中。

## 对比多个 Session

`view tr` 可以在不依赖 peval workspace 的情况下对比已保留的 session。每个输入
session 会成为报告中的一个 Trial。报告先显示 report notes、可筛选的
Leaderboard、Trajectory Overview，然后显示选中的 Trial trajectory。对比 JSON
只保存一份规范的 `leaderboard.entries` 行列表，并刻意省略 benchmark/task
矩阵字段以及旧版重复的 heatmap/table 行列表。
Leaderboard 的 `duration_ms` 表示 active agent/tool work time，会排除超过 10
分钟的已保留 session 空闲间隔。原始首末事件跨度会作为 `wall_duration_ms` 保留在
Trial metadata 和 leaderboard 行中。

对比 JSONL session：

```bash
peval-py view tr -a opencode \
  -p session-a.jsonl \
  -p session-b.jsonl \
  -o
```

对比 Psychevo DB session：

```bash
peval-py view tr \
  -d ~/.psychevo/state.db \
  -s <session-a> \
  -s <session-b> \
  -o
```

对比不同 adapter 拥有的 DB：

```bash
peval-py view tr \
  -d ~/.hermes/state.db \
  -d ~/.local/share/opencode/opencode.db \
  -a d1=hermes \
  -a d2=opencode \
  -o
```

`-a ADAPTER` 会作为所有输入的默认 adapter。某个 path 或 DB 需要单独指定
adapter 时，使用 `-a pN=ADAPTER` 或 `-a dN=ADAPTER`。path 和 DB 的索引都从
1 开始，并且分别计数。

如果 adapter 配置了 `[adapters.<id>].default_db_path`，可以用 `-d @<id>` 代替
手动输入 DB 路径。这个 token 也会把该 DB 输入绑定到同一个 adapter。
`-d @psychevo -a d1=opencode` 会因为 selector 与 token adapter 冲突而失败。

多个 DB 输入需要显式 session 时，用 `-s dN=<session-id>` 绑定到对应 DB：

```bash
peval-py view tr \
  -d ~/.hermes/state.db \
  -d ~/.local/share/opencode/opencode.db \
  -a d1=hermes \
  -a d2=opencode \
  -s d1=<hermes-session-id> \
  -s d2=<opencode-session-id> \
  -o
```

`view tr` 也可以混合 path 和 DB 输入。`export tr` 仍然只支持单 session。

添加轻量备注：

```bash
peval-py view tr \
  -d ~/.psychevo/state.db \
  -s <session-a> \
  -s <session-b> \
  -n 0="Report context" \
  -n 2="Session B follow-up" \
  -o
```

`-n/--note 0=...` 是报告级备注。正数索引绑定到命令中按顺序排列的 session，
索引从 1 开始。重复 `-n/--note` 会按 CLI 顺序追加。HTML 中的 report notes、
Leaderboard note snippets 和选中 Trial notes 使用与 `peval view` 一致的展示方式。

使用 `--source-alias N=TEXT` 可以给展开后的第 N 个输入 session 配置显示别名。
Input table 中也可以用 `alias`、`label` 或 `source_alias` 列写同样的值。
Alias 只提升 Leaderboard 和 source list 可读性，不会改变
`trajectory.session_id`、`trial_key`、`source_key`、`data_ref.relative_path`，
也不会替换 Evidence/Input Source 中的原始路径。

## Serve UI 布局

需要给 `serve` 使用本地 saved workspace 时，先运行一次 `init`：

```bash
peval-py init --root .local/peval-py
peval-py serve --root .local/peval-py
```

`peval-py init` 只创建 `peval-py serve` 真正需要的文件：`peval-py.toml` 和
`state.db`。已有的有效 `peval-py.toml` state DB 路径会被保留；它不会创建
`peval.toml`、`runs/`、`datasets/`、`scripts/`、eval template、
`$PSYCHEVO_HOME/peval-config.toml` 或 `.gitignore`。需要机器可读输出时使用
`--json`。`serve` 使用显式 `--root`、`PEVAL_ROOT`，或从当前目录向上发现
`peval-py.toml`；这个环境变量只是复用 root override 名称，不代表依赖 Rust
`peval` workspace。

静态 HTML 报告仍然是规范的离线报告。`peval-py serve` 复用同一套报告主体，
而不是另做一个 dashboard：Report Notes、Leaderboard、Trajectory Overview 和
选中 Trial trajectory 会保持静态报告中的顺序和样式。

静态 `view tr --format html` 报告继续使用固定 CDN ECharts script。`serve`
采用 local-first：页面优先加载 `/assets/echarts/6.0.0/echarts.min.js`，
本地脚本失败时才回退到同一个 CDN URL。serve route 会读取
`<workspace>/.cache/echarts/6.0.0/echarts.min.js`；cache miss 时用 Python
标准库下载固定 ECharts `6.0.0` CDN 资源，原子写入后再返回缓存内容。

Serve UI mode 只在共享主体周围增加 web-only 控件。页面顶部显示紧凑的
source/status toolbar，并通过 modal 管理 Session/ATIF path、SQLite DB、input
table、JSONL upload、ATIF JSON upload 和 report JSON upload。Path 和 DB 字段支持
一次粘贴多个路径；路径中包含空格时需要用引号包住。`C:\...`、`D:\...` 这类
Windows drive path 和 UNC path 会被保留，不会错误拼到 workspace root 下；当
`serve` 运行在 POSIX 且存在匹配的 `/mnt/<drive>/...` 路径时，会使用这个 WSL 风格路径。
Adapter 控件是每个表单 action row 里的紧凑下拉单选框，放在 add/upload 操作旁边，
默认是 `auto`，也就是沿用 CLI 的推断/默认 adapter 规则。导入失败会显示后端错误，
并且不会保存为 source。Source 可以 archive 以便恢复，也可以只从 peval-py state 中
delete；delete 不会删除原始文件或 database。对于可刷新的 source，selected Trial 的
Notes section 可以编辑匹配 peval cell 下的 `notes.md`；snapshot 上传来源保持只读。
Source 导入表单和 Timeline diagnostics section 使用融入报告主体的透明 shell，输入框和菜单仍保持可读的实底。

Source Manager 会在 DB 表单中暴露 adapter 的默认 DB 配置。选择带默认 DB path
的 adapter 后，可以不重复输入路径就 inspect 或 import。Source add/upload 表单也
支持 alias，每个已保存 source 行都有 alias 编辑器。Alias 与 source identity 分开
存储，并且可以清空。

Leaderboard 在 web UI 中可以增加用于导出选择的行复选框，并在 section header 右侧
提供一个 `Export` 菜单，包含 Table、JSON Report 和 HTML Report。点击行仍然选择
Trial；点击复选框只影响导出范围。导出时，如果当前可见行里有已勾选行，就导出
这些行；否则导出当前筛选和排序后的可见行。JSON 和 HTML 导出与 table 导出使用
同一行范围。点击 Export 或 table filter 菜单外部区域会自动收起菜单。Trajectory
Overview 的长节点序列会换行显示；有用时的节点会按当前 Trial 内最慢 step 以更低
对比度 10 档背景颜色深浅表达相对耗时，便于快速发现慢节点且不增加文字标签。
Timeline Waterfall 和 Timeline Detail Table section 可以折叠，点击 user/system
marker 或计时行会打开对应的 Step 详情抽屉。

对于 SQLite DB source，modal 内提供 Inspect 流程。输入或粘贴单个 DB 路径，可选
adapter，然后点击 Inspect DB。没有显式 adapter 时，`serve` 使用与 `view tr -d`
相同的 path-token adapter 推断；`.hermes/`、`.psychevo/` 或 `.opencode/` 路径会
推断为对应 adapter。如果路径无法推断或同时匹配多个 adapter，需要手动选择
adapter 后重新 inspect。勾选的 sessions 会保存为独立、可刷新的 sources，因此每个
session 都可以单独 archive、delete 或 refresh。

## Cached Analysis 与 Cell Notes

当 peval-py 能确定 workspace root 时，`view tr` 和 `serve` refresh 会只读地尝试
读取 peval cell 的 cached analysis，不会修改原始 trajectory。读取路径为：

```text
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.json
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.md
```

`analysis_eval_slug` 默认为 `default`。`<session-id>` 使用报告中的 session id；
`<agent-id>` 优先使用输入侧 `agent_name`，没有时使用 effective adapter id。
只有匹配 session 目录下恰好有一个 cell 目录包含 `analysis.json` 或 `analysis.md`
时才会采用；同一个 cell 目录里两者同时存在时，会合并 JSON summary 和 Markdown
report。缺失、JSON 格式错误、Markdown 无法读取或多个 cell 同时匹配时都会静默省略
对应内容。

JSON report 会把匹配结果写入 `annotations.analysis[]`，包含兼容旧消费者的
`relative_path`、可选 JSON 顶层 `summary`、可选 Markdown `md_report`，以及按格式
区分的 `relative_paths`。HTML 的 selected Trial 区域仅在存在 cached analysis 时
显示 Analysis section。`serve` refresh 会把 enrichment 写入持久化 report
snapshot，所以外部 `analysis.json` 或 `analysis.md` 改动后需要 Refresh 才会更新页面。

peval-py 也会从同一个 task 目录树读取 peval cell manual notes：

```text
<workspace>/runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/notes.md
```

`notes.md` 是 Trial note，不属于 Analysis。只有恰好一个 cell 目录包含
`notes.md` 时才会采用，并写入 `annotations.notes[]`：`source = "cell"`、label 为
`notes.md`、包含 Markdown 正文和相对 `source_ref`。Cell notes 会排在同一个 Trial
的 CLI 或 input-table notes 前面。

在 `serve` 中，`Edit notes` / `Add notes` 会为可刷新 source 写入这个 cell-local
`notes.md`，并立即刷新该 source snapshot。如果当前没有 note cell，peval-py 会写到
唯一 analysis cell 同级；如果也没有 cell，则创建 `peval-py-notes/notes.md`。多个
note 或 analysis cell 同时匹配时会失败且不写入。

## 本地化 HTML 报告

HTML 报告默认使用英文 UI。要把报告标题和对比区 UI 切换为简体中文，可在 `-c`
配置中设置：

```toml
[defaults]
locale = "zh-CN"
```

如果需要 workspace-local 默认值，也可以在 `peval-py.toml` 顶层设置：

```toml
state_db = "state.db"
locale = "zh-CN"
analysis_eval_slug = "default"
```

显式 `-c` 文件会 overlay `peval-py.toml`；`-c` 中没有写到的 key 会保留 workspace
值。

`zh` 是 `zh-CN` 的别名，`en-US` 会规范化为 `en`。locale 只从配置读取，不提供
CLI flag。`serve` 顶部 toolbar 提供 English/简体中文选择器；它会把顶层
`locale` 写入 `<workspace>/peval-py.toml`，更新正在运行的 server config，然后
刷新页面，让内嵌 i18n 文案重新加载。静态报告仍然由渲染时的配置控制。简体中文
报告中，Run、Result、Notes、Evidence、Steps/events、Session、variant、
evaluator、reasoning、selected trial trajectory、Turns、Tool Calls、
tool success / total、cache read、cache write 等领域术语保留英文；最后的选中
Trial Steps 明细区也保留英文。

## 常用 Flags

- `-p, --path PATH`：读取 session 路径。导出的 ATIF JSON 不需要 adapter；内置
  adapter 会按 JSONL 处理其他路径，自定义 path adapter 可以解析自己的文件格式。
- `-d, --db PATH`：读取 adapter 拥有的 SQLite database。配合 `view tr` 可以重复
  传入，用于跨 DB 对比。`-d @adapter` 会展开该 adapter 配置的
  `default_db_path`。
- `-s, --session-id ID`：选择 DB session。只有一个 DB 时，裸 `-s ID` 仍然有效并
  且可以重复。使用 `-s #N` 按列表序号选择；多个 DB 时使用 `-s dN=ID` 或
  `-s dN=#M`。
- `--list, -l`：打印 DB session 序号、id 和名称，然后退出。
- `--list-interactive, -li`：提示输入 `1,3-4` 或 `all` 这类序号选择，然后渲染选中
  sessions。
- `-a, --adapter ADAPTER`：选择默认的内置 adapter 或已安装的 adapter entry
  point。也可以重复传入 `pN=ADAPTER` 或 `dN=ADAPTER`，为单个输入覆盖 adapter。
- `-f, --format json|html`：强制报告格式。
- `-o, --output [PATH]`：写入文件。裸 `-o` 会为 export 写入
  `trajectory-<adapter>-<session>.json`，为 HTML view 写入
  `report-<adapter>-<session>.html`，或在 `-f json` 时写入
  `report-<adapter>-<session>.json`。多 session view 使用
  `report-<adapter>-sessions-<count>.<format>`；如果输入使用了多个 adapter，则使用
  `report-multi-adapter-sessions-<count>.<format>`。
- `-n, --note N=TEXT`：为 `view tr` 添加报告备注或 session 备注。`0` 表示报告，
  `1..N` 表示对应顺序的 session。
- `--source-alias N=TEXT`：为 `view tr` 或 `serve` 中展开后的第 N 个输入 session
  添加仅用于显示的别名。
- `--max-content-chars N`：限制大型 message 或 tool payload 的显示长度。
- `--no-redact`：关闭默认 secret redaction。

默认情况下，报告会隐藏明显包含 secret 的 key、authorization header、bearer token
和常见 `token=...` 文本。数值类 token 和 accounting totals 会保留。

## 阅读报告

HTML 报告会显示选中的 Trial/session、运行与结果摘要、可选备注与用量明细证据，
以及可见的 trajectory steps。匹配到的 tool observation 会显示在发起 tool call 的
Agent step 内。失败的 tool call 会使用红色 tool chip，并继续留在同一个 Agent step
里。

step token chip 会优先使用来源提供的真实 per-step metrics。如果某个 step 有可见
文本但没有 per-step token metrics，HTML 报告会显示带 `≈` 前缀和 tooltip 的
estimated chip。如果运行环境安装了 `tiktoken`，`peval-py` 会用它计算这个 HTML
估算值；否则回退到确定性的 byte-length 估算。这些估算只用于可视化，不会写入 ATIF
或 report JSON。

Steps 里的时间 chip 在有 timing metadata 时会显示轻量的占比填充。step duration、
elapsed time 和 tool execution time 分别按选中 Trial 内的同类时间缩放；其中
elapsed time 会优先按保留的 wall duration 缩放。因此它只是视觉提示，不是新的报告
指标。

如果 tool result 找不到对应的 tool call，`peval-py` 会把它保留为单独的 observation
step，并在报告中记录 conversion warning。

`export tr` 只支持单 session。重复 `-p`、重复 `-d`、重复 `-s` 或混合 path/DB
输入只用于 `view tr`。
