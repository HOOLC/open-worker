import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import sys
sys.path.insert(0, str(Path(__file__).parent / 'lib'))
from build_env import build_environment, settings
spec = importlib.util.spec_from_file_location('cache_budget', Path(__file__).parent / 'build/cache_budget.py')
cache = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cache)


class BuildSettingsTest(unittest.TestCase):
    def test_defaults_and_precedence(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            self.assertEqual(build_environment(root, {})['CARGO_TARGET_DIR'], str(root / 'target'))
            (root / '.env').write_text('ZORK_BUILD_ROOT="cache space"\nCARGO_TARGET_DIR=explicit\nSECRET=hidden\n')
            self.assertEqual(build_environment(root, {})['CARGO_TARGET_DIR'], str(root / 'explicit'))
            self.assertNotIn('SECRET', settings(root, {}))
            self.assertEqual(build_environment(root, {'CARGO_TARGET_DIR': 'override'})['CARGO_TARGET_DIR'], str(root / 'override'))

    def test_variant_and_no_execution(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / '.env').write_text("ZORK_BUILD_ROOT='$(touch forbidden)'\n")
            self.assertEqual(build_environment(root, {}, 'android')['CARGO_TARGET_DIR'], str(root / '$(touch forbidden)' / 'android'))
            self.assertFalse((root / 'forbidden').exists())

    def test_mount_guard(self):
        with patch('build_env.os.path.ismount', return_value=False):
            with self.assertRaises(ValueError):
                build_environment(Path('/absent'), {'ZORK_BUILD_MOUNT': '/missing'})

    def test_candidate_scope(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            for name in ['target', 'source', 'isolated/one']:
                p = root / name; p.mkdir(parents=True); (p / '.rustc_info.json').write_text('{}')
            (root / 'isolated/link').symlink_to(root / 'source', target_is_directory=True)
            self.assertEqual(set(cache.candidates(root)), {root / 'target', root / 'isolated/one'})

    def test_budget_validation(self):
        for env in [{'ZORK_BUILD_BUDGET_GIB':'nan'}, {'ZORK_BUILD_LOW_WATER_GIB':'101'}]:
            with self.assertRaises(ValueError): cache.budget(env)

    def test_preview_never_cleans(self):
        import io
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            target = root / 'target'; target.mkdir()
            (target / '.rustc_info.json').write_text('{}')
            env = {'ZORK_BUILD_ROOT': str(root), 'ZORK_BUILD_BUDGET_GIB': '1',
                   'ZORK_BUILD_LOW_WATER_GIB': '0.5'}
            with patch('sys.argv', ['cache_budget']), patch('sys.stdout', new_callable=io.StringIO), \
                 patch('cache_budget.build_environment', return_value=env), \
                 patch('cache_budget.size', return_value=2**31), \
                 patch('cache_budget.latest_write', return_value=0), \
                 patch('cache_budget.subprocess.run') as run:
                cache.main()
                run.assert_not_called()
            self.assertTrue(target.exists())

    def test_open_files_fail_closed(self):
        from subprocess import CompletedProcess
        with patch('cache_budget.shutil.which', return_value='/bin/lsof'), patch('cache_budget.subprocess.run', return_value=CompletedProcess([], 1, '', 'permission denied')):
            with self.assertRaises(RuntimeError): cache.idle(Path('/tmp/test'))


if __name__ == '__main__':
    # Enable patch() resolution for dynamically loaded module.
    sys.modules['cache_budget'] = cache
    unittest.main()
