import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import Mock, patch

import daily


SHA = "a" * 40
OTHER = "b" * 40


def release(sha=SHA, date="20261019", draft=False):
    return {"tag_name": f"daily-{date}-{sha}", "draft": draft, "prerelease": True, "target_commitish": sha,
            "published_at": date, "assets": [{"name": n, "size": 1} for n in daily.required_assets(sha)]}


class DailyTests(unittest.TestCase):
    def test_first_skip_and_new_commit(self):
        self.assertTrue(daily.plan(SHA, [])["build"])
        previous = release()
        self.assertFalse(daily.plan(SHA, [previous])["build"])
        result = daily.plan(OTHER, [previous])
        self.assertTrue(result["build"])
        self.assertEqual(result["baseline"], previous["tag_name"])

    def test_failed_draft_and_missing_assets_do_not_advance_baseline(self):
        good = release(date="20261018")
        draft = release(OTHER, draft=True)
        broken = release(OTHER)
        broken["assets"].pop()
        result = daily.plan(OTHER, [draft, broken, good])
        self.assertTrue(result["build"])
        self.assertEqual(result["baseline"], good["tag_name"])

    def test_duplicate_date_and_older_same_sha_are_skipped(self):
        self.assertFalse(daily.plan(SHA, [release(OTHER, "20261020"), release()])["build"])

    def test_pe_and_macho_architecture(self):
        with tempfile.TemporaryDirectory() as temp:
            for target, (_, machine) in daily.TARGETS.items():
                binary = Path(temp) / target
                if "windows" in target:
                    data = bytearray(128)
                    data[:2] = b"MZ"
                    struct.pack_into("<I", data, 60, 80)
                    data[80:84] = b"PE\0\0"
                    struct.pack_into("<H", data, 84, machine)
                else:
                    data = b"\xcf\xfa\xed\xfe" + struct.pack("<I", machine)
                binary.write_bytes(data)
                daily.inspect_binary(binary, target)
                opposite = next(t for t in daily.TARGETS if ("windows" in t) == ("windows" in target) and t != target)
                with self.assertRaises(ValueError):
                    daily.inspect_binary(binary, opposite)

    def test_windows_dynamic_crt_refused(self):
        with patch("daily.run", return_value="    bcryptprimitives.dll\n    icuuc.dll\n"):
            info = daily.dependencies(Path("app.exe"), "aarch64-pc-windows-msvc")
            self.assertEqual(info["imports"], ["bcryptprimitives.dll", "icuuc.dll"])
        with patch("daily.run", return_value="    KERNEL32.dll\n    VCRUNTIME140.dll\n"):
            with self.assertRaisesRegex(ValueError, "非系統 DLL"):
                daily.dependencies(Path("app.exe"), "x86_64-pc-windows-msvc")

    def test_mac_external_dylib_refused(self):
        with patch("daily.run", return_value="app:\n    /opt/homebrew/lib/example.dylib (compatibility version 1.0.0)\n"):
            with self.assertRaises(ValueError):
                daily.dependencies(Path("app"), "aarch64-apple-darwin")

    def assets(self, directory):
        for target in daily.TARGETS:
            name = daily.names(SHA, target)
            archive = directory / (name + ".zip")
            archive.write_bytes(b"archive-test")
            checksum = daily.digest(archive)
            (directory / (name + ".json")).write_text(json.dumps({"sha": SHA, "target": target, "sha256": checksum}))
            (directory / (name + ".sha256")).write_text(f"{checksum}  {name}.zip\n")

    def test_wrong_source_or_corrupt_asset_refuses_publish_before_network(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            self.assets(directory)
            daily.validate_assets(directory, SHA)
            next(directory.glob("*.zip")).write_bytes(b"corrupt")
            with patch("daily.gh") as remote:
                with self.assertRaises(ValueError):
                    daily.publish("o/r", SHA, "20261019", directory, "")
                remote.assert_not_called()

    def test_upload_failure_never_publishes_and_draft_is_reusable(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            self.assets(directory)
            def fail_upload(*args):
                if args[:2] == ("release", "upload"):
                    raise RuntimeError("network failure")
                return ""
            with patch("daily.releases", return_value=[]), patch("daily.gh", side_effect=fail_upload) as remote:
                with self.assertRaises(RuntimeError):
                    daily.publish("o/r", SHA, "20261019", directory, "")
                self.assertFalse(any("--draft=false" in c.args for c in remote.call_args_list))
            draft = release(draft=True)
            for asset in draft["assets"]:
                local = directory / asset["name"]
                asset.update(size=local.stat().st_size, digest="sha256:" + daily.digest(local))
            with patch("daily.releases", return_value=[draft]), patch("daily.gh", return_value="") as remote:
                daily.publish("o/r", SHA, "20261019", directory, "")
                self.assertFalse(any(c.args[:2] == ("release", "create") for c in remote.call_args_list))
                self.assertEqual(sum("--draft=false" in c.args for c in remote.call_args_list), 1)


    def test_zip_size_boundary_and_oversize_before_any_network(self):
        fake = Mock(name="archive.zip")
        fake.stat.return_value.st_size = 50 * 1024 * 1024
        daily.check_zip_size(fake)
        fake.stat.return_value.st_size += 1
        with self.assertRaisesRegex(ValueError, "50 MiB"):
            daily.check_zip_size(fake)
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            self.assets(directory)
            # 實際檔案上限調小，驗證 publish 在任何網路存取前拒絕超標。
            with patch("daily.MAX_ZIP_BYTES", 1), patch("daily.gh") as remote:
                with self.assertRaisesRegex(ValueError, "50 MiB"):
                    daily.publish("o/r", SHA, "20261019", directory, "")
                remote.assert_not_called()

    def test_package_rejects_oversize_before_writing_manifest(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            binary = root / "app.exe"
            binary.write_bytes(b"fake binary")
            out = root / "dist"
            with patch("daily.inspect_binary"), patch("daily.dependencies", return_value={}), patch("daily.MAX_ZIP_BYTES", 1):
                with self.assertRaisesRegex(ValueError, "50 MiB"):
                    daily.package(binary, "x86_64-pc-windows-msvc", SHA, out)
            self.assertFalse(list(out.glob("*.json")))
            self.assertFalse(list(out.glob("*.sha256")))

    def history(self):
        rows = []
        for i in range(35):
            row = release(f"{i:040x}")
            row.update(id=i + 1, published_at=f"2026-10-19T00:00:{i // 2:02d}Z")
            rows.append(row)
        return rows

    def test_cleanup_keeps_latest_30_and_protects_other_releases(self):
        history = self.history()
        protected = [release(SHA), release(OTHER, draft=True), release("c" * 40), release("d" * 40)]
        protected[0]["prerelease"] = False
        protected[2]["tag_name"] = "v1.0.0"
        protected[3]["assets"].pop()
        rows = {f"repos/o/r/releases/{r['id']}": r for r in history}
        def api(*args):
            return json.dumps(rows[args[1]]) if len(args) == 2 else ""
        with patch("daily.releases", return_value=history + protected), patch("daily.gh", side_effect=api) as remote:
            daily.cleanup("o/r", f"{34:040x}")
            deleted = [c.args for c in remote.call_args_list if "DELETE" in c.args]
            self.assertEqual(deleted, [("api", "--method", "DELETE", f"repos/o/r/releases/{i}") for i in range(5, 0, -1)])

    def test_no_cleanup_until_source_is_published(self):
        with patch("daily.releases", return_value=[release(draft=True)]), patch("daily.gh") as remote:
            with self.assertRaisesRegex(ValueError, "尚未完整發布"):
                daily.cleanup("o/r", SHA)
            remote.assert_not_called()

    def test_cleanup_rechecks_before_delete(self):
        history = self.history()
        changed = dict(history[4], prerelease=False)
        with patch("daily.releases", return_value=history), patch("daily.gh", return_value=json.dumps(changed)) as remote:
            with self.assertRaisesRegex(ValueError, "狀態已變更"):
                daily.cleanup("o/r", f"{34:040x}")
            self.assertFalse(any("DELETE" in c.args for c in remote.call_args_list))

    def test_cleanup_failure_retries_without_republishing(self):
        history = self.history()[-31:]
        old = history[0]
        def fail_delete(*args):
            if "DELETE" in args:
                raise RuntimeError("delete failed")
            return json.dumps(old)
        with patch("daily.releases", return_value=history), patch("daily.gh", side_effect=fail_delete):
            with self.assertRaisesRegex(RuntimeError, "delete failed"):
                daily.cleanup("o/r", f"{34:040x}")
        with patch("daily.releases", return_value=history), patch("daily.gh", return_value=json.dumps(old)) as remote:
            daily.cleanup("o/r", f"{34:040x}")
            self.assertEqual(sum("DELETE" in c.args for c in remote.call_args_list), 1)
            self.assertFalse(any("release" in c.args for c in remote.call_args_list))
        with patch("daily.releases", return_value=history[1:]), patch("daily.gh") as remote:
            daily.cleanup("o/r", f"{34:040x}")
            remote.assert_not_called()


if __name__ == "__main__":
    unittest.main()
