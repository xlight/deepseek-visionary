#!/usr/bin/env python3
"""发版前统一 bump 版本号，覆盖仓库全部版本载体。

用法:
    python3 scripts/bump_version.py <new-version> [--release]

参数:
    new-version  新版本号（x.y.z，不允许 v 前缀）
    --release    自动执行 git add + commit + tag + push（一键发布）

行为:
    1. 校验版本号格式（x.y.z，不允许 v 前缀）
    2. 更新 Cargo.toml 中 [workspace.package] 的 version
    3. 更新 Cargo.lock 中 visionary-server / visionary-zed-ext 两个包的 version
    4. 更新 crates/visionary-zed-ext/extension.toml 的 version
    5. 更新 packages/dsh-plugin/package.json 的 version（DSH 原生插件包；
       发布 workflow dsh-plugin-release.yml 要求 tag == package.json
       == workspace Cargo.toml 一致）
    6. 更新 packages/dsh-plugin/lib/index.mjs 的 COMPAT_MINOR 常量
       （与 Rust 二进制 minor 锁步；verify 校验，漏改即 fail）
    7. 更新 server.json 的 version 与 5 个平台 mcpb 下载 URL（MCP Registry
       元数据；fileSha256 是构建产物哈希，发布后运行
       `python3 scripts/update_server_json.py <version> v<version> dist/`
       用实际 .mcpb 计算）
    8. 校验所有版本载体最终一致
    9. 不带 --release 时只打印后续步骤；带 --release 时自动执行
       git add → commit → tag vX.Y.Z → push origin HEAD --tags

目的:
    保证打 tag 前所有版本号一致（Zed 扩展 package check 要求
    extensions.toml 登记的 version == submodule 内 extension.toml 的 version；
    cargo-dist 要求 tag == v{workspace version}；dsh-plugin release
    workflow 校验 tag == package.json == workspace Cargo.toml）。

示例:
    python3 scripts/bump_version.py 0.3.0            # 只 bump + 打印步骤
    python3 scripts/bump_version.py 0.3.0 --release  # bump + commit + tag + push
"""
import json
import re
import subprocess
import sys

VERSION_RE = re.compile(r"^\d+\.\d+\.\d+$")

CARGO_TOML = "Cargo.toml"
CARGO_LOCK = "Cargo.lock"
EXTENSION_TOML = "crates/visionary-zed-ext/extension.toml"
PLUGIN_JSON = "packages/dsh-plugin/package.json"
SERVER_JSON = "server.json"

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
    version_pattern = r'(?m)^version\s*=\s*"\d+\.\d+\.\d+"'
    if not re.search(version_pattern, section):
        print(f"error: [workspace.package] 段中未找到 version 字段", file=sys.stderr)
        sys.exit(1)
    new_section = re.sub(
        version_pattern,
        f'version = "{new_version}"',
        section,
        count=1,
    )

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


def bump_plugin_json(new_version: str) -> None:
    """DSH 原生插件包 package.json（dsh-plugin-release.yml 要求与 workspace 一致）。"""
    with open(PLUGIN_JSON, encoding="utf-8") as f:
        data = json.load(f)

    old = data.get("version")
    if not isinstance(old, str) or not VERSION_RE.match(old):
        print(f"error: {PLUGIN_JSON} 中 version 无效: {old!r}", file=sys.stderr)
        sys.exit(1)
    data["version"] = new_version
    with open(PLUGIN_JSON, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"updated {PLUGIN_JSON} -> {new_version}")


# 插件源码中与 Rust 二进制 minor 锁步的兼容版本常量（不匹配时所有工具结果附带版本警告）
PLUGIN_INDEX_MJS = "packages/dsh-plugin/lib/index.mjs"
COMPAT_MINOR_RE = re.compile(r'const COMPAT_MINOR = "\d+\.\d+"')


def bump_compat_minor(new_version: str) -> None:
    """同步插件源码 COMPAT_MINOR 常量为 new_version 的 major.minor 前缀。"""
    minor = ".".join(new_version.split(".")[:2])
    with open(PLUGIN_INDEX_MJS, encoding="utf-8") as f:
        text = f.read()

    new_text, n = COMPAT_MINOR_RE.subn(
        f'const COMPAT_MINOR = "{minor}"', text, count=1
    )
    if n != 1:
        print(f"error: {PLUGIN_INDEX_MJS} 中未找到 COMPAT_MINOR 常量", file=sys.stderr)
        sys.exit(1)
    with open(PLUGIN_INDEX_MJS, "w", encoding="utf-8") as f:
        f.write(new_text)
    print(f"updated {PLUGIN_INDEX_MJS} [COMPAT_MINOR] -> {minor}")


def bump_server_json(new_version: str) -> None:
    """MCP Registry 元数据：version + 5 平台下载 URL。

    fileSha256 是构建产物哈希，发布后由 update_server_json.py
    用实际 .mcpb 计算——此处不伪造。
    """
    with open(SERVER_JSON, encoding="utf-8") as f:
        data = json.load(f)

    data["version"] = new_version
    old_tag = f"/v{re.escape(data['version'])}/"
    for pkg in data.get("packages", []):
        ident = pkg.get("identifier", "")
        # 替换任意 /vX.Y.Z/ 段为 /v{new_version}/
        new_ident = re.sub(r"/v\d+\.\d+\.\d+/", f"/v{new_version}/", ident)
        if new_ident != ident:
            pkg["identifier"] = new_ident

    with open(SERVER_JSON, "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"updated {SERVER_JSON} -> {new_version} (version + 下载 URL)")


def verify_consistency(new_version: str) -> None:
    """校验所有版本载体最终与 new_version 一致。"""
    checks = []

    cargo = open(CARGO_TOML, encoding="utf-8").read()
    m = re.search(r"\[workspace\.package\][^\[]*?version\s*=\s*\"([^\"]+)\"", cargo, re.S)
    checks.append((CARGO_TOML, m.group(1) if m else None))

    lock = open(CARGO_LOCK, encoding="utf-8").read()
    for name in LOCK_PACKAGES:
        m = re.search(
            rf'\[\[package\]\]\nname = "{re.escape(name)}"\nversion = "([^"]+)"',
            lock,
        )
        checks.append((f"{CARGO_LOCK} [{name}]", m.group(1) if m else None))

    ext = open(EXTENSION_TOML, encoding="utf-8").read()
    m = re.search(r'(?m)^version\s*=\s*"([^"]+)"', ext)
    checks.append((EXTENSION_TOML, m.group(1) if m else None))

    plugin = json.load(open(PLUGIN_JSON, encoding="utf-8"))
    checks.append((PLUGIN_JSON, plugin.get("version")))

    index = open(PLUGIN_INDEX_MJS, encoding="utf-8").read()
    m = COMPAT_MINOR_RE.search(index)
    # COMPAT_MINOR 是 major.minor 前缀，与 new_version 的前两段比较
    checks.append((f"{PLUGIN_INDEX_MJS} [COMPAT_MINOR]", m.group(0).split('"')[1] if m else None))

    server = json.load(open(SERVER_JSON, encoding="utf-8"))
    checks.append((SERVER_JSON, server.get("version")))

    expected_minor = ".".join(new_version.split(".")[:2])
    ok = True
    for name, version in checks:
        expected = expected_minor if "COMPAT_MINOR" in name else new_version
        flag = "ok" if version == expected else "MISMATCH"
        if version != expected:
            ok = False
        print(f"  [{flag}] {name}: {version}")
    if not ok:
        print(f"error: 版本不一致，请检查上述 MISMATCH 项", file=sys.stderr)
        sys.exit(1)
    print(f"version consistency: all {len(checks)} files match {new_version}")


def git_release(new_version: str) -> None:
    """一键发布：add → commit → tag → push。"""
    files = [
        CARGO_TOML, CARGO_LOCK, EXTENSION_TOML, PLUGIN_JSON,
        SERVER_JSON, PLUGIN_INDEX_MJS,
        "packages/dsh-plugin/pnpm-lock.yaml",
    ]
    tag = f"v{new_version}"

    subprocess.run(["git", "add", *files], check=True)
    print("git add:", ", ".join(files))

    subprocess.run(
        ["git", "commit", "-m", f"Bump version to {new_version}"], check=True
    )
    print(f"git commit: 'Bump version to {new_version}'")

    # tag 若已存在则失败（避免覆盖旧 tag）
    subprocess.run(["git", "tag", tag], check=True)
    print(f"git tag: {tag}")

    subprocess.run(["git", "push", "origin", "HEAD", "--tags"], check=True)
    print(f"git push: origin HEAD + {tag}")


def main() -> None:
    args = [a for a in sys.argv[1:] if a != "--release"]
    release = "--release" in sys.argv[1:]
    if len(args) != 1:
        print(__doc__)
        sys.exit(2)

    new_version = args[0]
    if not VERSION_RE.match(new_version):
        print(f"error: 版本号格式应为 x.y.z，收到 '{new_version}'", file=sys.stderr)
        sys.exit(2)

    bump_cargo_toml(new_version)
    bump_cargo_lock(new_version)
    bump_extension_toml(new_version)
    bump_plugin_json(new_version)
    bump_compat_minor(new_version)
    bump_server_json(new_version)
    print()
    verify_consistency(new_version)

    if release:
        print()
        print("执行一键发布：")
        git_release(new_version)
        print()
        print("完成。push 触发以下 CI：")
        print("  - cargo-dist 二进制发布（release.yml，v* tag）")
        print("  - Zed 扩展同步（zed-extension-release.yml，v* tag）")
        print("  - npm 发布 @xlight-oss/visionary-dsh（dsh-plugin-release.yml，v* tag）")
        print()
        print("发布后待办：")
        print("  1. server.json 的 fileSha256 由 update-server-json workflow")
        print("     （workflow_run 监听 Release 完成）自动回填，无需手动操作；")
        print("     若未触发，可手动兜底：")
        print(f"     gh release download v{new_version} --pattern '*.mcpb' --dir dist --clobber")
        print(f"     python3 scripts/update_server_json.py {new_version} v{new_version} dist/")
    else:
        print()
        print("完成。后续发布步骤：")
        print(f"  1. 复核版本一致性输出（已校验全部 7 个版本条目：Cargo.toml / Cargo.lock×2 / extension.toml / package.json / COMPAT_MINOR / server.json）")
        print(f"  2. git add Cargo.toml Cargo.lock {EXTENSION_TOML} {PLUGIN_JSON} {SERVER_JSON} {PLUGIN_INDEX_MJS} packages/dsh-plugin/pnpm-lock.yaml")
        print(f"  3. git commit -m 'Bump version to {new_version}'")
        print(f"  4. git tag v{new_version} && git push origin main --tags")
        print("     （tag 必须指向本次 commit，确保 extension.toml 版本与 tag 一致）")
        print(f"  5. 发布后：python3 scripts/update_server_json.py {new_version} "
              f"v{new_version} dist/（更新 server.json 的 fileSha256）")
        print(f"  或直接一条命令：python3 scripts/bump_version.py {new_version} --release")


if __name__ == "__main__":
    main()
