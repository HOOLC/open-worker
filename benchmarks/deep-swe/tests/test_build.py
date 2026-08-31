import unittest
from pathlib import Path

from zork_deepswe.build import docker_build_command


class BuildZorkAgentTest(unittest.TestCase):
    def test_builds_a_linux_amd64_export_from_the_repository(self) -> None:
        command = docker_build_command(
            Path("/repo"), Path("/project/zork-agent.Dockerfile"), Path("/output")
        )

        self.assertEqual(command[:3], ["docker", "buildx", "build"])
        self.assertIn("linux/amd64", command)
        self.assertIn("/project/zork-agent.Dockerfile", command)
        self.assertEqual(command[-1], "/repo")


if __name__ == "__main__":
    unittest.main()
