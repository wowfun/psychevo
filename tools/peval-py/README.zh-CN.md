# peval-py

语言：简体中文 | [English](README.md)

`peval-py` 是 `peval` 的轻量 Python 版本，用于查看已保留的 agent
轨迹。它读取 JSONL session 或 adapter 拥有的 SQLite database，并输出 ATIF JSON
或静态 peval 风格报告。

## 从 Checkout 安装

用 `uv` 安装本地 Python 工具：

```bash
uv tool install --editable ./tools/peval-py
```

之后可以直接使用短命令：

```bash
peval-py --help
peval-py view tr --help
```

也可以不安装，直接从源码树运行：

```bash
uv run --project tools/peval-py peval-py --help
```

## 构建本地二进制

`peval-py` 使用 `pandas` 为 inspect 模式提供表格化分析；`uv` 会根据
`tools/peval-py/pyproject.toml` 安装该运行时依赖。读取 `.xlsx` 输入清单是可选能力，
运行时需要 `openpyxl`。请在目标操作系统和 CPU 架构上构建。生成物建议放在
`.local/` 下，仓库会忽略这个目录。

PyInstaller 是最简单的单文件打包方式：

```bash
cd /path/to/psychevo

uv run --project tools/peval-py --with pyinstaller pyinstaller \
  --onefile \
  --name peval-py \
  --paths tools/peval-py/src \
  --distpath .local/peval-py-build/dist \
  --workpath .local/peval-py-build/work \
  --specpath .local/peval-py-build/spec \
  tools/peval-py/src/peval_py/cli.py
```

运行打包后的命令，并用 fixture 生成一份报告做检查：

```bash
.local/peval-py-build/dist/peval-py --help

.local/peval-py-build/dist/peval-py view tr \
  -m raw \
  -a opencode \
  -p tools/peval-py/tests/fixtures/common_session.jsonl \
  -o .local/peval-py-build/report.json

python3 -m json.tool .local/peval-py-build/report.json >/dev/null
```

Nuitka 也是一种选择，适合想做 compiled-Python 构建并且本机有 C 编译器的场景。
选择前建议在目标平台上比较输出大小和启动表现。

## 使用指南

用 `-a ADAPTER` 为所有输入设置默认 adapter。生成对比报告时，可以重复传入
`-a pN=ADAPTER` 或 `-a dN=ADAPTER`，让单个 path 或 DB 输入使用不同 adapter。

`view tr` 默认使用 bounded inspect 模式，适合先探索大文件：

```bash
peval-py view tr -a opencode -p session.jsonl
```

Inspect 输出是固定的紧凑 JSON digest，包含 session 身份、token totals、秒级
active duration、step/tool duration distributions、最耗时或 token-heavy 的行，以及
可用的 tool errors。`--head` 和 `--tail` 默认都是 2，`--top` 默认是 5；
`--steps <ids>` 会加入指定 step 证据，并支持 `1,3:5` 这样的逗号和 range
selector；`--tool-call <tool_call_id>` 可独立显示对应 tool call 及其匹配的 tool
result。`--max-content-chars` 会限制 inspect preview 文本长度。裸 `-o` 会写入
带时间戳的报告文件，并在 stdout 打印保存路径。

需要完整 peval JSON 或 HTML report 时使用 `-m raw`：

```bash
peval-py view tr -m raw -a opencode -p session.jsonl -f html -o report.html
```

Raw report 模式还接受 `--agent-name`、`--agent-version`、`--model` 和
`--no-redact` 这类转换/展示覆盖；默认 inspect 模式会拒绝这些 flags。

adapter TOML 表可以设置 `default_db_path`；相对路径按定义该值的 TOML 文件解析。
使用 `-d @adapter` 可以展开这个默认 DB 路径，并把该 DB 输入绑定到同一个 adapter。

当需要从 workspace 外读取已有 peval-py workspace 的 `peval-py.toml` 时，可以在
`view tr` 或 `export tr` 中使用 `-r, --root DIR`。这会选择 workspace 配置，例如
locale、`analysis_eval_slug`、adapter defaults 和 `default_db_path`；不会初始化或
修改 workspace。如果该目录还没有 `peval-py.toml`，请先运行 `peval-py init -r DIR`。

```bash
peval-py view tr -r .local/peval-py -d @opencode --list
peval-py export tr -r .local/peval-py -d @opencode -s <session-id> -o
```

当输入更适合放在 CSV、JSON 或 `.xlsx` 清单中维护时，使用
`-i, --input-table PATH`。表格每一行都会展开成同一份报告中的一个 session。
直接传入的 `-p/--path` 和 `-d/--db` 会先加载，然后按文件顺序追加表格行。
相对 `path` 和 `db` 会按清单文件所在目录解析。`.xlsx` 只在本机已安装
`openpyxl` 时可用。

使用 `--source-alias N=TEXT`，或 input table 的 `alias`/`label`/`source_alias`
列，可以给来源添加仅用于显示的名称。Alias 只提升报告可读性，不改变 session id、
trial key、source identity 或 Evidence/Input Source 路径。在 Leaderboard 中，
canonical Session 列保持不变，别名显示在独立的 Session Alias 列。

在对比报告中，Leaderboard 的 Duration 列和 JSON `duration_ms` 字段表示 active
agent/tool work time。已保留 session 中较长的空闲间隔会单独保存在
`wall_duration_ms` 字段中。Leaderboard 和 `serve` Source Manager 也会显示来自
`trajectory_meta.finished_at_ms` 的 Last Turn End。

当通过 `view tr -r <workspace>` 选择 workspace root，或从当前目录向上发现
workspace 时，报告还会尝试读取 peval cell cached analysis：
`runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.json`
和 `analysis.md`。默认 slug 是 `default`；匹配到的 summary 和 Markdown report 会
显示在 selected Trial 的 Analysis section，并写入 JSON `annotations.analysis[]`。

同一个 task 目录树也可以提供 manual Trial notes：
`runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/notes.md`。这些内容会写入
JSON `annotations.notes[]`，并排在 CLI/table notes 前面。在 `peval-py serve` 中，
可刷新的 source 可以编辑或添加这个 cell-local `notes.md`；snapshot 上传来源保持只读。
Serve 展示已保存 snapshot 时，会在 active report 组合阶段叠加当前 workspace 里的
`analysis.json`、`analysis.md` 和 `notes.md`；因此 reload 或 Refresh 即使遇到原始
source DB/file 无法成功刷新，也能显示 notes/analysis 的更新。

`peval-py serve` 保持静态报告继续使用 CDN，但在 serve 页面中会优先从
`<workspace>/.cache/echarts/6.0.0/echarts.min.js` 提供 ECharts，本地脚本失败时
回退到固定 CDN URL。Source Manager 会暴露配置好的默认 DB path、source alias 编辑，
Last Turn End 排序，并提供 English/简体中文选择器；语言选择会把顶层 `locale`
持久化到 `peval-py.toml`。Path 来源字段也可以输入另一个 workspace root、
`runs/`、`runs/<analysis_eval_slug>`，或 Trial cell 上层目录；serve 会递归导入完整
cell 到当前 workspace 作为 snapshot，并保持外部 workspace 不变。

CSV 示例：

```csv
path,db,session_id,adapter,alias,n,report_note,agent_name,agent_version,model
runs/hermes.jsonl,,,,Hermes source,Hermes row note,跨 agent 对比,Hermes,,deepseek-v4-flash
,state.db,ses_123,opencode,OpenCode source,OpenCode row note,,,,
```

生成一份多 session HTML 报告：

```bash
peval-py view tr \
  -m raw \
  -a psychevo \
  -i inputs.csv \
  -f html \
  -o report.html
```

JSON 清单可以是顶层 array，也可以是带有 `rows` 和 `report_notes` 的对象：

```json
{
  "report_notes": ["本地跨 agent 对比。"],
  "rows": [
    {"path": "runs/hermes.jsonl", "adapter": "hermes", "alias": "Hermes source", "note": "Hermes row"},
    {"db": "opencode.db", "session_id": "ses_123", "adapter": "opencode", "source_alias": "OpenCode source"}
  ]
}
```

`export tr -i` 展开后仍只能有一个 session。多行清单请使用 `view tr -i`。

报告生成、session 对比和自定义 adapter 示例见
[peval-py 轻量轨迹报告](../../docs/i18n/zh-CN/evaluation/peval-py.md)。
