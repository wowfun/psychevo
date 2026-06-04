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

`peval-py` 的运行时只依赖 Python 标准库，可以打包成本地可执行文件。请在目标
操作系统和 CPU 架构上构建。生成物建议放在 `.local/` 下，仓库会忽略这个目录。

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

报告生成、session 对比和自定义 adapter 示例见
[peval-py 轻量轨迹报告](../../docs/i18n/zh-CN/evaluation/peval-py.md)。
