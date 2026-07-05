from __future__ import annotations

from pathlib import Path
from urllib.request import urlopen

from peval_py.serve.errors import HttpError
from peval_py.state import ServeStateStore

ECHARTS_VERSION = "6.0.0"
ECHARTS_ASSET_PATH = f"/assets/echarts/{ECHARTS_VERSION}/echarts.min.js"
ECHARTS_CDN_URL = f"https://cdn.jsdelivr.net/npm/echarts@{ECHARTS_VERSION}/dist/echarts.min.js"


def cached_echarts_asset(store: ServeStateStore) -> bytes:
    path = echarts_cache_path(store)
    if path.is_file():
        return path.read_bytes()
    try:
        data = download_echarts_asset()
    except Exception as exc:  # noqa: BLE001 - HTTP asset boundary.
        raise HttpError(502, f"failed to cache ECharts: {exc}") from exc
    if not data:
        raise HttpError(502, "failed to cache ECharts: empty response")
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.with_name(path.name + ".tmp")
    tmp_path.write_bytes(data)
    tmp_path.replace(path)
    return data


def echarts_cache_path(store: ServeStateStore) -> Path:
    return store.paths.root / ".cache" / "echarts" / ECHARTS_VERSION / "echarts.min.js"


def download_echarts_asset() -> bytes:
    with urlopen(ECHARTS_CDN_URL, timeout=15) as response:  # noqa: S310 - fixed URL.
        return response.read()
