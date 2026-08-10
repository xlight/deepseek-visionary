#!/usr/bin/env python3
"""发版前统一 bump 版本号：workspace Cargo.toml + Cargo.lock + extension.toml。

用法:
    python3 scripts/bump_version.py <new-version>

行为:
    1. 校验版本号格式（x.y.z，不允许 v 前缀）
    2. 更新 Cargo.toml 中 [workspace.package] 的 version
    3. 更新 Cargo.lock 中 visionary-server / visionary-zed-ext 两个包的 version
    4. 更新 crates/visionary-zed-ext/extension.toml 的 version
    5. 打印后续发布步骤提示

目的:
    保证打 tag 前所有版本号一致（Zed 扩展 package check 要求
    extensions.toml 登记的 version == submodule 内 extension.toml 的 version；
    cargo-dist 要求 tag == v{workspace version}）。

示例:
    python3 scripts/bump_version.py 0.3.0
"""
import re
import sys

VERSION_RE = re.compile(r"^\d+\.\d+\.\d+$")

CARGO_TOML = "Cargo.toml"
CARGO_LOCK = "Cargo.lock"
EXTENSION_TOML = "crates/visionary-zed-ext/extension.toml"

# Cargo.lock 中需要同步版本的 workspace 包
LOCK_PACKAGES = ["visionary-server", "visionary-zed-ext"]


def bump_cargo_toml(new_version: str) -> None:
    """只改 [workspace.package] 段的 version，避免误改 dependencies 里的版本。"""
    with open(CARGO_TOML, encoding="utf-8") as f:
        text = f.read()

    match = re.search(r"\[workspace\.package\](.*?)\n\[", text, re.S)
    if not match:
        print(f"error: 找不到 [workspace.package] 段", file=sys.stderr)
        sys.exit(1)

    section = match.group(1)
    new_section = re.sub(
        r'(?m)^version\s*=\s*"\d+\.\d+\.\d+"',
        f'version = "{new_version}"',
        section,
        count=1,
    )
    if new_section == section:
        print(f"error: [workspace.package] 段中未找到 version 字段", file=sys.stderr)
        sys.exit(1)

    text = text[: match.start(1)] + new_section + text[match.end(1):]
    with open(CARGO_TOML, "w", encoding="utf-8") as f:
        f.write(text)
    print(f"updated {CARGO_TOML} -> {new_version}")


def bump_cargo_lock(new_version: str) -> None:
    """按包名定位 [[package]] 块并更新其 version。"""
    with open(CARGO_LOCK, encoding="utf-8") as f:
        text = f.read()

    for name in LOCK_PACKAGES:
        pattern = re.compile(
            rf'(\[\[package\]\]\nname = "{re.escape(name)}"\n)version = "\d+\.\d+\.\d+"'
        )
        new_text, n = pattern.subn(
            rf'\g<1>version = "{new_version}"', text, count=1
        )
        if n != 1:
            print(f"error: Cargo.lock 中找不到 {name} 包", file=sys.stderr)
            sys.exit(1)
        text = new_text
        print(f"updated {CARGO_LOCK} [{name}] -> {new_version}")

    with open(CARGO_LOCK, "w", encoding="utf-8") as f:
        f.write(text)


def bump_extension_toml(new_version: str) -> None:
    with open(EXTENSION_TOML, encoding="utf-8") as f:
        text = f.read()

    new_text, n = re.subn(
        r'(?m)^version\s*=\s*"\d+\.\d+\.\d+"',
        f'version = "{new_version}"',
        text,
        count=1,
    )
    if n != 1:
        print(f"error: {EXTENSION_TOML} 中未找到 version 字段", file=sys.stderr)
        sys.exit(1)

    with open(EXTENSION_TOML, "w", encoding="utf-8") as f:
        f.write(new_text)
    print(f"updated {EXTENSION_TOML} -> {new_version}")


def main() -> None:
    if len(sys.argv) != 2:
        print(__doc__)
        sys.exit(2)

    new_version = sys.argv[1]
    if not VERSION_RE.match(new_version):
        print(f"error: 版本号格式应为 x.y.z，收到 '{new_version}'", file=sys.stderr)
        sys.exit(2)

    bump_cargo_toml(new_version)
    bump_cargo_lock(new_version)
    bump_extension_toml(new_version)

    print()
    print("完成。后续发布步骤：")
    print(f"  1. git add Cargo.toml Cargo.lock {EXTENSION_TOML}")
    print(f"  2. git commit -m 'Bump version to {new_version}'")
    print(f"  3. git tag v{new_version} && git push origin main --tags")
    print(f"     （tag 必须指向本次 commit，确保 extension.toml 版本与 tag 一致）")


if __name__ == "__main__":
    main()
