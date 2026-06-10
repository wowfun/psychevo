# peval-py 轻量轨迹报告

语言：简体中文 | [English](../../../evaluation/peval-py.md)

`peval-py` 是 peval 的轻量 Python 版本，用于查看已保留的 agent 轨迹。
它读取 JSONL session 或 adapter 拥有的 SQLite database，并输出派生的 ATIF
轨迹或静态 peval 风格报告。它不会运行 agent、给任务打分，也不会修改 peval
workspace。

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

运行方式和内置 adapter 相同：

```bash
peval-py view tr -c custom.toml -p custom-session.log -o
peval-py export tr -a custom -p custom-session.log -o
```

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

## 从 Hermes DB 生成报告

`hermes` adapter 可以直接读取当前 Hermes SQLite 持久化格式。通过 `--db`
传入 Hermes database 路径。如果省略 `--session-id`，adapter 会选择最近活跃的
session。存在 `sessions.system_prompt` 时，报告会把它作为第一条 system step。

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

## Serve UI 布局

静态 HTML 报告仍然是规范的离线报告。未来的 `peval-py serve` web UI 复用同一套
报告主体，而不是另做一个 dashboard：Report Notes、Leaderboard、Trajectory
Overview 和选中 Trial trajectory 会保持静态报告中的顺序和样式。

Serve UI mode 只在共享主体周围增加 web-only 控件。导入区默认折叠，位于报告标题
上方。Leaderboard 在 web UI 中可以增加用于导出选择的行复选框，并在 section
header 右侧提供分区导出控件。点击行仍然选择 Trial；点击复选框只影响导出范围。
导出时，如果当前可见行里有已勾选行，就导出这些行；否则导出当前筛选和排序后的
可见行。JSON 和 HTML 导出与 CSV 表格导出使用同一行范围。

## 本地化 HTML 报告

HTML 报告默认使用英文 UI。要把报告标题和对比区 UI 切换为简体中文，在配置中设置：

```toml
[defaults]
locale = "zh-CN"
```

`zh` 是 `zh-CN` 的别名，`en-US` 会规范化为 `en`。locale 只从配置读取，不提供
CLI flag。简体中文报告中，Run、Result、Notes、Evidence、Steps/events、
Session、variant、evaluator、reasoning、selected trial trajectory、Turns、
Tool Calls、tool success / total、cache read、cache write 等领域术语保留英文；
最后的选中 Trial Steps 明细区也保留英文。

## 常用 Flags

- `-p, --path PATH`：读取 session 路径。导出的 ATIF JSON 不需要 adapter；内置
  adapter 会按 JSONL 处理其他路径，自定义 path adapter 可以解析自己的文件格式。
- `-d, --db PATH`：读取 adapter 拥有的 SQLite database。配合 `view tr` 可以重复
  传入，用于跨 DB 对比。
- `-s, --session-id ID`：选择 DB session。只有一个 DB 时，裸 `-s ID` 仍然有效并
  且可以重复。多个 DB 时使用 `-s dN=ID`。
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
