python3 - <<'PY'
import importlib.util
from pathlib import Path

path = Path("add.py")
spec = importlib.util.spec_from_file_location("target", path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

assert module.add(2, 3) == 5
assert module.add(-2, 4) == 2
PY
