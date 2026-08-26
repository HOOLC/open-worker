import tempfile
import unittest
from pathlib import Path

from scripts.benchmarks.prepare_deepswe import collect_unique_images


class PrepareDeepSweTest(unittest.TestCase):
    def test_collects_images_once_in_task_order(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tasks = []
            for name, image in (
                ("a", "registry/a:1"),
                ("b", "registry/b:1"),
                ("c", "registry/a:1"),
            ):
                task = root / name
                task.mkdir()
                (task / "task.toml").write_text(
                    f'[environment]\ndocker_image = "{image}"\n'
                )
                tasks.append(task)

            self.assertEqual(
                collect_unique_images(tasks),
                ["registry/a:1", "registry/b:1"],
            )

    def test_runner_prepulls_before_starting_pier(self) -> None:
        runner = (Path(__file__).parent / "run-deepswe.sh").read_text()
        self.assertLess(runner.index("prepare_deepswe.py"), runner.index("pier run"))


if __name__ == "__main__":
    unittest.main()
