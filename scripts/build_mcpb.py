#!/usr/bin/env python3
"""构建 MCPB 包：把 visionary-server 二进制 + manifest.json 打成 .mcpb（zip）。

用法:
    python3 scripts/build_mcpb.py <target> <version> <binary_path> <output_dir>

示例:
    python3 scripts/build_mcpb.py aarch64-apple-darwin 0.1.4 \\
        target/aarch64-apple-darwin/release/visionary-server dist

产物:
    dist/visionary-server-<target>.mcpb
"""
import json
import os
import stat
import sys
import zipfile

TARGET = sys.argv[1]
VERSION = sys.argv[2]
BINARY = sys.argv[3]
OUT_DIR = sys.argv[4]

BIN_NAME = os.path.basename(BINARY)  # visionary-server 或 visionary-server.exe


def build_manifest() -> dict:
    return {
        "manifest_version": "0.3",
        "name": "deepseek-visionary",
        "display_name": "DeepSeek Visionary",
        "version": VERSION,
        "description": (
            "DeepSeek web vision model as an MCP server: upload images and analyze them "
            "with the native multimodal model, with browser auto-login."
        ),
        "author": {"name": "xlight"},
        "repository": {
            "type": "git",
            "url": "https://github.com/xlight/deepseek-visionary",
        },
        "license": "MIT",
        "keywords": ["deepseek", "vision", "multimodal", "image", "mcp"],
        "privacy_policies": ["https://chat.deepseek.com/privacy"],
        "server": {
            "type": "binary",
            "entry_point": BIN_NAME,
            "mcp_config": {
                "command": BIN_NAME,
                "args": [],
                "env": {},
            },
        },
        "compatibility": {
            "platforms": ["darwin", "linux", "win32"],
        },
        "tools_generated": True,
    }


def main() -> None:
    os.makedirs(OUT_DIR, exist_ok=True)
    out_path = os.path.join(OUT_DIR, f"visionary-server-{TARGET}.mcpb")

    manifest = build_manifest()
    mode = os.stat(BINARY).st_mode

    with zipfile.ZipFile(out_path, "w", zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("manifest.json", json.dumps(manifest, indent=2, ensure_ascii=False))

        zi = zipfile.ZipInfo(BIN_NAME)
        # 保留 unix 可执行权限位（0o755），Windows 客户端会自行处理 .exe
        zi.external_attr = (stat.S_IFREG | (mode & 0o777)) << 16
        zi.compress_type = zipfile.ZIP_DEFLATED
        with open(BINARY, "rb") as f:
            zf.writestr(zi, f.read())

    print(f"built {out_path} ({os.path.getsize(out_path)} bytes)")
    # 输出 sha256（registry server.json 需要 fileSha256）
    import hashlib

    with open(out_path, "rb") as f:
        digest = hashlib.sha256(f.read()).hexdigest()
    print(f"sha256 {digest}")


if __name__ == "__main__":
    main()
