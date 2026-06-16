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

`peval-py` 默认运行路径只依赖 Python 标准库，可以打包成本地可执行文件。读取
`.xlsx` 输入清单是可选能力，运行时需要 `openpyxl`。请在目标操作系统和 CPU 架构
上构建。生成物建议放在 `.local/` 下，仓库会忽略这个目录。

PyInstaller 是最简单的单文件打包方式：

```bash
cd /path/to/psychevo

uvx pyinstaller \
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

当输入更适合放在 CSV、JSON 或 `.xlsx` 清单中维护时，使用
`-i, --input-table PATH`。表格每一行都会展开成同一份报告中的一个 session。
直接传入的 `-p/--path` 和 `-d/--db` 会先加载，然后按文件顺序追加表格行。
相对 `path` 和 `db` 会按清单文件所在目录解析。`.xlsx` 只在本机已安装
`openpyxl` 时可用；如果希望保持标准库路径，请另存为 CSV。

在对比报告中，Leaderboard 的 Duration 列和 JSON `duration_ms` 字段表示 active
agent/tool work time。已保留 session 中较长的空闲间隔会单独保存在
`wall_duration_ms` 字段中。

当 peval-py 能确定 workspace root 时，报告还会尝试读取 peval cell cached
analysis：`runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/analysis.json`
和 `analysis.md`。默认 slug 是 `default`；匹配到的 summary 和 Markdown report 会
显示在 selected Trial 的 Analysis section，并写入 JSON `annotations.analysis[]`。

同一个 task 目录树也可以提供 manual Trial notes：
`runs/<analysis_eval_slug>/<agent-id>/<session-id>/<cell_key>/notes.md`。这些内容会写入
JSON `annotations.notes[]`，并排在 CLI/table notes 前面。在 `peval-py serve` 中，
可刷新的 source 可以编辑或添加这个 cell-local `notes.md`；snapshot 上传来源保持只读。

CSV 示例：

```csv
path,db,session_id,adapter,n,report_note,agent_name,agent_version,model
runs/hermes.jsonl,,,,Hermes row note,跨 agent 对比,Hermes,,deepseek-v4-flash
,state.db,ses_123,opencode,OpenCode row note,,,,
```

生成一份多 session HTML 报告：

```bash
peval-py view tr \
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
    {"path": "runs/hermes.jsonl", "adapter": "hermes", "note": "Hermes row"},
    {"db": "opencode.db", "session_id": "ses_123", "adapter": "opencode"}
  ]
}
```

`export tr -i` 展开后仍只能有一个 session。多行清单请使用 `view tr -i`。

报告生成、session 对比和自定义 adapter 示例见
[peval-py 轻量轨迹报告](../../docs/i18n/zh-CN/evaluation/peval-py.md)。
