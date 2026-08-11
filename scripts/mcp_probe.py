#!/usr/bin/env python3
"""驱动 visionary-server 做 MCP stdio 测试。

双子命令 CLI：
    python3 scripts/mcp_probe.py smoke <binary-path>
        stdio initialize 握手 + tools/list 断言 4 个工具；非零退出码表示失败。
    python3 scripts/mcp_probe.py analyze <image-path> [<prompt>]
        对指定图片调用 deepseek_vision 真实识图并输出结果与耗时（保留原有测图逻辑）。

rmcp stdio 帧格式 = 换行分隔 JSON（newline-delimited），不是 Content-Length。
"""
import base64
import json
import subprocess
import sys
import time
from queue import Queue, Empty
import threading

SERVER = "target/debug/visionary-server"

EXPECTED_TOOLS = {
    "deepseek_vision",
    "deepseek_vision_status",
    "deepseek_vision_login",
    "deepseek_vision_logout",
}


# Windows 上 Python 默认 stdout 编码是 cp1252，无法输出中文（MCP 响应含中文 instructions），
# 统一强制 UTF-8（对 UTF-8 环境无副作用）。
for _stream in (sys.stdout, sys.stderr):
    if _stream is not None and hasattr(_stream, "reconfigure"):
        _stream.reconfigure(encoding="utf-8", errors="replace")


class LineReader:
    """后台线程读 stdout 行，放进队列，带超时。"""

    def __init__(self, proc):
        self.q = Queue()
        self.proc = proc
        self._t = threading.Thread(target=self._run, daemon=True)
        self._t.start()

    def _run(self):
        for raw in self.proc.stdout:
            line = raw.decode("utf-8", "replace").strip()
            if line:
                self.q.put(line)

    def get(self, timeout: float) -> str:
        return self.q.get(timeout=timeout)


def send(proc, payload: dict):
    body = json.dumps(payload, ensure_ascii=False) + "\n"
    proc.stdin.write(body.encode("utf-8"))
    proc.stdin.flush()


def spawn(binary: str):
    return subprocess.Popen(
        [binary, "mcp-stdio"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        # stderr 保持独立：server 日志走 stderr，stdout 是纯净的 MCP 协议通道
        stderr=None,
    )


def cmd_smoke(binary: str) -> int:
    """stdio initialize 握手 + tools/list 断言 4 工具。任一失败返回非零。"""
    if not binary:
        print("error: smoke 需要 <binary-path> 参数", file=sys.stderr)
        return 2

    proc = spawn(binary)
    reader = LineReader(proc)

    # --- initialize ---
    send(proc, {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "mcp-probe", "version": "0.1"},
        },
    })
    t0 = time.time()
    try:
        resp = json.loads(reader.get(30))
    except Empty:
        print(f"[initialize] TIMEOUT after 30s for {binary}", file=sys.stderr)
        proc.kill()
        return 1
    print(f"[initialize] {time.time() - t0:.2f}s -> {json.dumps(resp, ensure_ascii=False)[:200]}")
    if "result" not in resp:
        print(f"[initialize] FAILED: {json.dumps(resp, ensure_ascii=False)[:200]}", file=sys.stderr)
        proc.kill()
        return 1

    send(proc, {"jsonrpc": "2.0", "method": "notifications/initialized"})

    # --- tools/list ---
    send(proc, {"jsonrpc": "2.0", "id": 2, "method": "tools/list"})
    try:
        resp = json.loads(reader.get(30))
    except Empty:
        print("[tools/list] TIMEOUT after 30s", file=sys.stderr)
        proc.kill()
        return 1

    tools = resp.get("result", {}).get("tools", [])
    names = {t.get("name") for t in tools}
    print(f"[tools/list] {len(tools)} tools: {sorted(names)}")

    missing = EXPECTED_TOOLS - names
    if missing:
        print(
            f"[tools/list] FAILED: missing tools {sorted(missing)}",
            file=sys.stderr,
        )
        proc.kill()
        return 1

    print("[smoke] OK: initialize 握手成功，4 个工具齐全")
    proc.kill()
    return 0


def cmd_analyze(image_path: str, prompt: str) -> int:
    """对图片调用 deepseek_vision 真实识图（保留原有测图工作流）。"""
    with open(image_path, "rb") as f:
        image_data = f.read()
    print(f"image: {image_path} ({len(image_data)} bytes)")

    proc = spawn(SERVER)
    reader = LineReader(proc)

    # --- initialize ---
    send(proc, {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": {"name": "mcp-probe", "version": "0.1"},
        },
    })
    t0 = time.time()
    resp = json.loads(reader.get(30))
    print(f"[initialize] {time.time() - t0:.2f}s -> {json.dumps(resp, ensure_ascii=False)[:200]}")

    send(proc, {"jsonrpc": "2.0", "method": "notifications/initialized"})

    # --- tools/call deepseek_vision ---
    b64 = base64.b64encode(image_data).decode("ascii")
    send(proc, {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "deepseek_vision",
            "arguments": {"image": b64, "prompt": prompt},
        },
    })
    t0 = time.time()
    try:
        resp = json.loads(reader.get(300))
    except Empty:
        print("[tools/call] TIMEOUT after 300s")
        proc.kill()
        return 1
    elapsed = time.time() - t0
    print(f"[tools/call] {elapsed:.2f}s")
    if "result" in resp and "content" in resp["result"]:
        for block in resp["result"]["content"]:
            print("--- content ---")
            print(block.get("text", "")[:4000])
        if resp["result"].get("isError"):
            print("!!! isError = True")
    elif "error" in resp:
        print("!!! error:", json.dumps(resp["error"], ensure_ascii=False))
    else:
        print(json.dumps(resp, ensure_ascii=False)[:3000])

    proc.kill()
    return 0


def main() -> int:
    if len(sys.argv) < 2:
        print(__doc__)
        return 2

    sub = sys.argv[1]
    if sub == "smoke":
        binary = sys.argv[2] if len(sys.argv) > 2 else ""
        return cmd_smoke(binary)
    if sub == "analyze":
        if len(sys.argv) < 3:
            print("error: analyze 需要 <image-path> 参数", file=sys.stderr)
            return 2
        image_path = sys.argv[2]
        prompt = sys.argv[3] if len(sys.argv) > 3 else "请描述这张图片的内容"
        return cmd_analyze(image_path, prompt)

    print(f"error: unknown subcommand `{sub}`（支持 smoke / analyze）", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main())
