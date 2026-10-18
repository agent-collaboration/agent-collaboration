import importlib.util
import unittest
import typing as t
from pathlib import Path

SCRIPT: t.Final[Path] = Path(__file__).resolve().parents[1] / "tooling/check-central-core-boundary.py"
SPEC = importlib.util.spec_from_file_location("central_core_boundary", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError("central boundary checker missing")
CHECKER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECKER)


@t.final
class CentralCoreBoundaryTests(unittest.TestCase):
    def test_static_lifetime_is_not_global_state(self) -> None:
        self.assertFalse(CHECKER.forbidden_core_construct("fn tag() -> &'static str { \"fleet\" }"))

    def test_actual_static_globals_remain_forbidden(self) -> None:
        for source in ("static CACHE: Cache = Cache::new();", "pub static mut CACHE: u64 = 0;"):
            with self.subTest(source=source):
                self.assertTrue(CHECKER.forbidden_core_construct(source))

    def test_async_and_platform_conditionals_remain_forbidden(self) -> None:
        for source in ("pub async fn load() {}", '#[cfg(target_arch = "wasm32")]', '#[cfg(not(target_arch = "wasm32"))]'):
            with self.subTest(source=source):
                self.assertTrue(CHECKER.forbidden_core_construct(source))


if __name__ == "__main__":
    unittest.main()
