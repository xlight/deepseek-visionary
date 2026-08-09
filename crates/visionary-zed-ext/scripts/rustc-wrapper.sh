#!/bin/sh
# Zed dev extension 构建用 rustc 代理脚本（被 .cargo/config.toml 的 build.rustc 引用）。
#
# cargo 以 `rustc <编译参数>` 的方式调用本脚本，`$@` 直接是编译参数。
# 优先转发到 rustup 安装的 rustc（自带 wasm32-wasip2 target 的 std），
# 找不到时回退到 PATH 中的 rustc。
#
# 目的：MacPorts / Homebrew 等系统 cargo 配套的 rustc 没有 wasm32-wasip2 的 std，
# Zed 安装 dev extension 时构建会报 `error[E0463]: can't find crate for 'core'`。
# 通过本脚本把编译固定到 rustup 的 rustc，与外部 PATH / 环境变量设置无关。
set -e
if [ -x "$HOME/.cargo/bin/rustc" ]; then
    exec "$HOME/.cargo/bin/rustc" "$@"
fi
exec rustc "$@"
