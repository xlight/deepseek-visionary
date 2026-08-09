#!/usr/bin/env python3
"""驱动 visionary-server 做 MCP stdio 手动测试，测量大图处理真实耗时。

用法:
    python3 scripts/mcp_probe.py <image_path> [<prompt>]

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


def main():
    image_path = sys.argv[1]
    prompt = sys.argv[2] if len(sys.argv) > 2 else "请描述这张图片的内容"

    with open(image_path, "rb") as f:
        image_data = f.read()
    print(f"image: {image_path} ({len(image_data)} bytes)")

    proc = subprocess.Popen(
        [SERVER],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        # stderr 保持独立：server 日志走 stderr，stdout 是纯净的 MCP 协议通道
        stderr=None,
    )
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
        # dump any stderr output seen so far
        sys.exit(1)
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


if __name__ == "__main__":
    main()
