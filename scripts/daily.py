"""四架構 daily 的規劃、封裝與發布；只使用 Python 標準函式庫。"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import stat
import struct
import subprocess
import tempfile
import zipfile


TARGETS = {
    "x86_64-pc-windows-msvc": ("windows-x64", 0x8664),
    "aarch64-pc-windows-msvc": ("windows-arm64", 0xAA64),
    "x86_64-apple-darwin": ("macos-intel-x64", 0x01000007),
    "aarch64-apple-darwin": ("macos-apple-silicon-arm64", 0x0100000C),
}
SHA = re.compile(r"[0-9a-f]{40}")
TAG = re.compile(r"daily-(\d{8})-([0-9a-f]{40})")


def run(*args):
    return subprocess.check_output(args, text=True, encoding="utf-8").strip()


def gh(*args):
    return run("gh", *args)


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def names(sha, target):
    return f"sysrqui-{TARGETS[target][0]}-{sha}"


def required_assets(sha):
    return {names(sha, t) + suffix for t in TARGETS for suffix in (".zip", ".json", ".sha256")}


def complete(release):
    match = TAG.fullmatch(release["tag_name"])
    assets = {a["name"] for a in release.get("assets", []) if a["size"] > 0}
    return bool(match and not release["draft"] and release["prerelease"]
                and required_assets(match[2]) <= assets)


def plan(sha, releases):
    if not SHA.fullmatch(sha):
        raise ValueError("source SHA 必須為完整 40 位小寫十六進位")
    successful = sorted((r for r in releases if complete(r)),
                        key=lambda r: r["published_at"], reverse=True)
    # 已成功發布過的相同 SHA 不因日期改變而重複發布。
    same = next((r for r in successful if TAG.fullmatch(r["tag_name"])[2] == sha), None)
    return {"sha": sha, "build": same is None,
            "baseline": successful[0]["tag_name"] if successful else ""}


def releases(repo):
    pages = json.loads(gh("api", f"repos/{repo}/releases?per_page=100", "--paginate", "--slurp"))
    return [release for page in pages for release in page]


def inspect_binary(binary, target):
    data = binary.read_bytes()
    if "windows" in target:
        if data[:2] != b"MZ" or len(data) < 64:
            raise ValueError("不是 PE 執行檔")
        offset = struct.unpack_from("<I", data, 60)[0]
        if data[offset:offset + 4] != b"PE\0\0":
            raise ValueError("PE signature 錯誤")
        machine = struct.unpack_from("<H", data, offset + 4)[0]
    else:
        if data[:4] != b"\xcf\xfa\xed\xfe":
            raise ValueError("需要單架構 64-bit Mach-O")
        machine = struct.unpack_from("<I", data, 4)[0]
    if machine != TARGETS[target][1]:
        raise ValueError(f"執行檔架構不符 {target}: {machine:#x}")


def dependencies(binary, target):
    if "windows" in target:
        report = run("dumpbin", "/dependents", str(binary))
        dlls = re.findall(r"^\s+([\w.-]+\.dll)\s*$", report, re.I | re.M)
        if not dlls:
            raise ValueError("dumpbin 未列出任何相依，拒絕無證據封裝")
        # Windows 自帶 DLL；CRT 靜態連結，避免依賴使用者另外安裝 VC runtime。
        system = {"kernel32.dll", "user32.dll", "gdi32.dll", "gdi32full.dll",
                  "advapi32.dll", "shell32.dll", "ole32.dll", "oleaut32.dll",
                  "comdlg32.dll", "comctl32.dll", "shlwapi.dll", "crypt32.dll",
                  "bcrypt.dll", "ncrypt.dll", "ws2_32.dll", "ntdll.dll", "bcryptprimitives.dll", "icuuc.dll",
                  "dwrite.dll", "d2d1.dll", "d3d11.dll", "dxgi.dll", "dcomp.dll",
                  "dwmapi.dll", "imm32.dll", "uxtheme.dll", "version.dll",
                  "propsys.dll", "winmm.dll", "secur32.dll", "winhttp.dll",
                  "iphlpapi.dll", "netapi32.dll", "userenv.dll", "msimg32.dll",
                  "usp10.dll", "rpcrt4.dll", "setupapi.dll", "cfgmgr32.dll",
                  "dbghelp.dll", "d3dcompiler_47.dll", "winspool.drv"}
        external = [d for d in dlls if d.lower() not in system
                    and not d.lower().startswith(("api-ms-win-", "ext-ms-win-"))]
        if external:
            raise ValueError(f"有未隨附的非系統 DLL: {external}")
        return {"imports": dlls, "minimum_os": "尚未取得真機最低版本驗收"}
    report = run("otool", "-L", str(binary))
    imports = [line.strip().split(" (", 1)[0] for line in report.splitlines()[1:]]
    if not imports or any(not p.startswith(("/usr/lib/", "/System/Library/")) for p in imports):
        raise ValueError(f"有未封裝的非系統 dylib: {imports}")
    load_commands = run("otool", "-l", str(binary))
    minimum = re.search(r"\bminos\s+([\d.]+)", load_commands)
    if not minimum:
        minimum = re.search(r"cmd LC_VERSION_MIN_MACOSX\s+cmdsize \d+\s+version ([\d.]+)", load_commands)
    if not minimum:
        raise ValueError("無法從 Mach-O 取得最低 macOS")
    return {"imports": imports, "minimum_os": minimum[1],
            "minimum_os_note": "僅二進位 deployment target；不代表真機相容性驗收"}


def package(binary, target, sha, output):
    if not SHA.fullmatch(sha):
        raise ValueError("source SHA 格式錯誤")
    inspect_binary(binary, target)
    info = dependencies(binary, target)
    output.mkdir(parents=True, exist_ok=True)
    name = names(sha, target)
    paths = [output / (name + suffix) for suffix in (".zip", ".json", ".sha256")]
    if any(p.exists() for p in paths):
        raise ValueError("輸出已存在，拒絕覆寫")
    metadata = {"schema": 1, "sha": sha, "target": target, "gui_verified": False,
                "checks": "See Actions run; package itself does not execute workspace verification",
                "signing": "unsigned" if "windows" in target else "ad-hoc only; not Developer ID signed or notarized",
                "dependencies": info}
    with tempfile.TemporaryDirectory() as temp:
        root = Path(temp) / name
        root.mkdir()
        if "windows" in target:
            shutil.copy2(binary, root / "sysrqui.exe")
        else:
            app = root / "SysRQUI.app" / "Contents"
            (app / "MacOS").mkdir(parents=True)
            executable = app / "MacOS" / "sysrqui"
            shutil.copy2(binary, executable)
            executable.chmod(0o755)
            with (app / "Info.plist").open("wb") as file:
                plistlib.dump({"CFBundleExecutable": "sysrqui", "CFBundleName": "SysRQUI",
                              "CFBundleIdentifier": "com.bonaparta.sysrqui",
                              "CFBundlePackageType": "APPL", "CFBundleVersion": "0.1.0",
                              "CFBundleShortVersionString": "0.1.0",
                              "LSMinimumSystemVersion": info["minimum_os"],
                              "NSHighResolutionCapable": True}, file)
            run("codesign", "--force", "--sign", "-", str(root / "SysRQUI.app"))
            run("codesign", "--verify", "--strict", str(root / "SysRQUI.app"))
        (root / "version.json").write_text(json.dumps(metadata, ensure_ascii=False, indent=2), encoding="utf-8")
        (root / "README.txt").write_text(
            f"SysRQUI 測試版\nsource SHA: {sha}\ntarget: {target}\n"
            "Windows：完整解壓後啟動 sysrqui.exe；macOS：完整解壓後開啟 SysRQUI.app。\n"
            "未提供正式簽章／公證，系統可能阻擋；若無法啟動，請回報系統訊息，不需停用系統安全機制。\n"
            "尚未完成 GUI／最低 OS 真機驗收。請確認顯示 Win／Mac，回報版本 SHA、CPU、OS、重現步驟與截圖。\n"
            "回報：https://github.com/bonaparta/SysJustRQUI/issues\n", encoding="utf-8")
        with zipfile.ZipFile(paths[0], "w", zipfile.ZIP_DEFLATED) as archive:
            for file in sorted(root.rglob("*")):
                if file.is_file():
                    entry = zipfile.ZipInfo.from_file(file, str(file.relative_to(root.parent)))
                    entry.create_system = 3
                    entry.external_attr = (stat.S_IFREG | (0o755 if file.name == "sysrqui" else 0o644)) << 16
                    archive.writestr(entry, file.read_bytes(), compress_type=zipfile.ZIP_DEFLATED)
    metadata.update({"archive": paths[0].name, "sha256": digest(paths[0])})
    paths[1].write_text(json.dumps(metadata, ensure_ascii=False, indent=2), encoding="utf-8")
    paths[2].write_text(f"{metadata['sha256']}  {paths[0].name}\n", encoding="ascii")


def validate_assets(directory, sha):
    actual = {p.name for p in directory.iterdir() if p.is_file()}
    if actual != required_assets(sha):
        raise ValueError(f"發布產物缺漏或多餘: {actual ^ required_assets(sha)}")
    for target in TARGETS:
        name = names(sha, target)
        meta = json.loads((directory / (name + ".json")).read_text(encoding="utf-8"))
        checksum = digest(directory / (name + ".zip"))
        if meta["sha"] != sha or meta["target"] != target or meta["sha256"] != checksum:
            raise ValueError("manifest SHA／target／checksum 不一致")
        if (directory / (name + ".sha256")).read_text().strip() != f"{checksum}  {name}.zip":
            raise ValueError("checksum 檔內容不一致")


def publish(repo, sha, date, directory, baseline):
    validate_assets(directory, sha)
    current = releases(repo)
    if not plan(sha, current)["build"]:
        print("此 SHA 已完整發布，跳過")
        return
    if not re.fullmatch(r"\d{8}", date):
        raise ValueError("日期格式錯誤")
    tag = f"daily-{date}-{sha}"
    existing = next((r for r in current if r["tag_name"] == tag), None)
    if existing and not existing["draft"]:
        raise ValueError("同名非 draft Release 不完整，拒絕覆寫")
    if existing and existing["target_commitish"] != sha:
        raise ValueError("既有 draft 的 source SHA 不符，拒絕復用")
    notes = (f"# SysRQUI daily 測試版 {date}\n\nSource SHA: `{sha}`\n\n"
             "提供 Windows x64／ARM64、macOS Intel x64／Apple Silicon arm64 四份 ZIP。"
             "所有產物來自相同 SHA，各 target 已通過 workspace fmt/build/test/clippy。\n\n"
             "下載與 CPU 相符的 ZIP 並完整解壓；Windows 啟動 sysrqui.exe，Mac 開啟 SysRQUI.app。"
             "每份附有 version.json、相依檢查與 SHA256；可下載 .sha256 核對完整性。\n\n"
             "未提供正式簽章／公證（Mac 僅 ad-hoc），GUI 及最低 OS 真機驗收待完成；"
             "系統可能阻擋啟動，請回報訊息，不需停用安全機制。"
             "請在 issue 提供 SHA、CPU、OS、重現步驟與截圖。\n\n")
    if baseline:
        notes += f"[相對上次成功 daily 的變更](https://github.com/{repo}/compare/{baseline}...{sha})\n"
    else:
        notes += f"首次 daily；[來源與歷史](https://github.com/{repo}/commits/{sha})\n"
    with tempfile.TemporaryDirectory() as temp:
        body = Path(temp) / "notes.md"
        body.write_text(notes, encoding="utf-8")
        if not existing:
            gh("release", "create", tag, "--repo", repo, "--target", sha, "--draft",
               "--prerelease", "--title", f"SysRQUI daily {date} {sha[:12]}", "--notes-file", str(body))
        else:
            gh("release", "edit", tag, "--repo", repo, "--notes-file", str(body))
        # 僅覆寫失敗嘗試的 draft assets；已發布版本永不覆寫或刪除。
        gh("release", "upload", tag, *[str(p) for p in sorted(directory.iterdir())],
           "--repo", repo, "--clobber")
        remote = next(r for r in releases(repo) if r["tag_name"] == tag)
        if {a["name"] for a in remote["assets"] if a["size"] > 0} != required_assets(sha):
            raise ValueError("遠端 assets 不完整，保留 draft 不發布")
        for asset in remote["assets"]:
            local = directory / asset["name"]
            if asset.get("digest") != "sha256:" + digest(local) or asset["size"] != local.stat().st_size:
                raise ValueError("遠端 asset checksum／大小不符，保留 draft 不發布")
        gh("release", "edit", tag, "--repo", repo, "--draft=false", "--prerelease", "--latest=false")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    p = sub.add_parser("plan")
    p.add_argument("--repo", required=True)
    p.add_argument("--sha", required=True)
    p = sub.add_parser("package")
    p.add_argument("--binary", type=Path, required=True)
    p.add_argument("--target", choices=TARGETS, required=True)
    p.add_argument("--sha", required=True)
    p.add_argument("--output", type=Path, required=True)
    p = sub.add_parser("publish")
    for arg in ("repo", "sha", "date"):
        p.add_argument("--" + arg, required=True)
    p.add_argument("--directory", type=Path, required=True)
    p.add_argument("--baseline", default="")
    args = parser.parse_args()
    if args.command == "plan":
        result = plan(args.sha, releases(args.repo))
        print(json.dumps(result))
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
                for key, value in result.items():
                    output.write(f"{key}={str(value).lower() if isinstance(value, bool) else value}\n")
    elif args.command == "package":
        package(args.binary, args.target, args.sha, args.output)
    else:
        publish(args.repo, args.sha, args.date, args.directory, args.baseline)


if __name__ == "__main__":
    main()
