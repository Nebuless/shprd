from __future__ import annotations

import unittest

from tests.test_report_verifier import VerifierTest
from report_discovery import inspect_artifact, source_conflicts
from report_io import git


class RepositoryIdentityTest(unittest.TestCase):
    setUp = VerifierTest.setUp

    def test_approved_when_origin_has_equivalent_url_suffix(self) -> None:
        self.revision["sourcePin"]["repository"] = (
            "https://github.com/DioxusLabs/dioxus.git"
        )
        git(
            self.source,
            "remote",
            "add",
            "origin",
            "https://github.com/DioxusLabs/dioxus/",
        )

        operation, detail = inspect_artifact(
            self.ledger["artifacts"][0], (self.source, self.target), None
        )

        self.assertEqual(operation["verdict"], "approved")
        self.assertEqual(
            source_conflicts(
                [detail],
                {
                    "repository": "https://github.com/DioxusLabs/dioxus/",
                    "sha": self.revision["sourcePin"]["sha"],
                },
                None,
            ),
            [],
        )
