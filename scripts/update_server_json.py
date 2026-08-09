#!/usr/bin/env python3
"""发布后同步更新仓库根 server.json（MCP Registry 元数据）。

用法:
    python3 scripts/update_server_json.py <version> <release-tag> <mcpb-dir>

参数:
    version      新版本号（如 0.2.0）
    release-tag  发布 tag（如 v0.2.0，用于拼接下载 URL）
    mcpb-dir     含 5 平台 .mcpb 文件的目录（从 GitHub Release 下载，或本地 dist/）

行为:
    读取 server.json，将 version 改为新版本，并把 5 个平台条目的 identifier
    与 fileSha256 更新为对应 .mcpb 的实际值。其他字段保持不变。

示例:
    python3 scripts/update_server_json.py 0.2.0 v0.2.0 dist/
"""
import hashlib
import json
import os
import sys

TARGETS = [
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
]


def sha256(path: str) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    if len(sys.argv) != 4:
        print(__doc__)
        sys.exit(2)

    version, release_tag, mcpb_dir = sys.argv[1], sys.argv[2], sys.argv[3]

    with open("server.json", encoding="utf-8") as f:
        data = json.load(f)

    data["version"] = version

    for pkg in data.get("packages", []):
        ident = pkg.get("identifier", "")
        # 匹配 mcpb 资产名（形如 .../visionary-server-<target>.mcpb）
        for target in TARGETS:
            marker = f"visionary-server-{target}.mcpb"
            if marker in ident:
                mcpb_path = os.path.join(mcpb_dir, marker)
                if not os.path.exists(mcpb_path):
                    print(f"error: missing {mcpb_path}", file=sys.stderr)
                    sys.exit(1)
                new_ident = (
                    f"https://github.com/xlight/deepseek-visionary/releases/"
                    f"download/{release_tag}/{marker}"
                )
                pkg["identifier"] = new_ident
                pkg["fileSha256"] = sha256(mcpb_path)
                print(f"updated {target}: sha256 {pkg['fileSha256'][:16]}...")
                break
        else:
            print(f"warning: no mcpb entry matched for {ident}", file=sys.stderr)

    with open("server.json", "w", encoding="utf-8") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"server.json updated to version {version}")


if __name__ == "__main__":
    main()
