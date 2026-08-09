//! DeepSeek Proof-of-Work challenge solver.
//!
//! 对照 Python 版 `pow.py` 逐行移植：通过 `wasmtime` 加载 DeepSeek 站内的
//! `sha3_wasm_bg.*.wasm`，调用导出函数 `wasm_solve` 求解 challenge。
//!
//! 与 Python 版的对应关系：
//! - `PoWSolver._init_hasher` → `WasmHasher::new`
//! - `PoWSolver._write_to_memory` → `WasmHasher::write_str`
//! - `PoWSolver._calculate_hash` → `WasmHasher::solve`
//! - `PoWSolver.solve_challenge` → `PoWSolver::solve_challenge`

use anyhow::{Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use wasmtime::{Engine, Instance, Linker, Memory, Module, Store};

/// 给 wasmtime 调用结果附加上下文并转为 anyhow 错误。
/// wasmtime 47 起 `wasmtime::Error` 不再实现 `std::error::Error`，
/// anyhow 的 `.context()` 不再适用，需先经 `From` 转换。
fn wasm_context<T>(result: std::result::Result<T, wasmtime::Error>, msg: &'static str) -> Result<T> {
    result.map_err(anyhow::Error::from).context(msg)
}

/// wasm 资产，编译期内嵌，随二进制分发（见 `assets/README.md`）。
const WASM_BYTES: &[u8] = include_bytes!("../../../assets/sha3_wasm_bg.7b9ca65ddd.wasm");

/// PoW challenge 配置，来自 `/api/v0/chat/create_pow_challenge` 响应
/// `data.biz_data.challenge` 字段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Challenge {
    pub algorithm: String,
    pub challenge: String,
    pub salt: String,
    pub difficulty: f64,
    pub expire_at: i64,
    pub signature: String,
    #[serde(default = "default_target_path")]
    pub target_path: String,
}

fn default_target_path() -> String {
    "/api/v0/chat/completion".to_string()
}

/// WASM 哈希器实例（对应 Python `_init_hasher` 的结果）。
struct WasmHasher {
    store: Store<()>,
    instance: Instance,
    memory: Memory,
}

impl WasmHasher {
    fn new() -> Result<Self> {
        let engine = Engine::default();
        let module = wasm_context(
            Module::new(&engine, WASM_BYTES),
            "failed to parse sha3_wasm_bg wasm module",
        )?;
        let mut store = Store::new(&engine, ());
        let linker = Linker::new(&engine);
        let instance = wasm_context(
            linker.instantiate(&mut store, &module),
            "failed to instantiate wasm module",
        )?;
        let memory = instance
            .get_memory(&mut store, "memory")
            .context("wasm module has no exported `memory`")?;
        Ok(Self {
            store,
            instance,
            memory,
        })
    }

    /// `__wbindgen_add_to_stack_pointer`：管理 wasm-bindgen 的栈指针。
    fn add_to_stack_pointer(&mut self, delta: i32) -> Result<i32> {
        let f = wasm_context(
            self.instance
                .get_typed_func::<(i32,), (i32,)>(&mut self.store, "__wbindgen_add_to_stack_pointer"),
            "missing `__wbindgen_add_to_stack_pointer` export",
        )?;
        Ok(f.call(&mut self.store, (delta,))?.0)
    }

    /// `__wbindgen_export_0`：wasm-bindgen 的线性内存分配器（malloc）。
    fn malloc(&mut self, size: usize, align: usize) -> Result<u32> {
        let f = wasm_context(
            self.instance
                .get_typed_func::<(u32, u32), (u32,)>(&mut self.store, "__wbindgen_export_0"),
            "missing `__wbindgen_export_0` export",
        )?;
        Ok(f.call(&mut self.store, (size as u32, align as u32))?.0)
    }

    /// 向 wasm 线性内存写入 UTF-8 字符串，返回 (ptr, len)。
    /// 对应 Python `_write_to_memory`。
    fn write_str(&mut self, text: &str) -> Result<(u32, u32)> {
        let bytes = text.as_bytes();
        let ptr = self.malloc(bytes.len(), 1)?;
        let data = self.memory.data_mut(&mut self.store);
        let start = ptr as usize;
        data[start..start + bytes.len()].copy_from_slice(bytes);
        Ok((ptr, bytes.len() as u32))
    }

    /// 执行 `wasm_solve`，返回 answer（status==0 时无解返回 None）。
    /// 对应 Python `_calculate_hash`。
    fn solve(&mut self, challenge: &str, prefix: &str, difficulty: f64) -> Result<Option<i64>> {
        let (challenge_ptr, challenge_len) = self.write_str(challenge)?;
        let (prefix_ptr, prefix_len) = self.write_str(prefix)?;

        let retptr = self.add_to_stack_pointer(-16)?;

        let wasm_solve = wasm_context(
            self.instance
                .get_typed_func::<(i32, u32, u32, u32, u32, f64), ()>(&mut self.store, "wasm_solve"),
            "missing `wasm_solve` export",
        )?;
        wasm_solve.call(
            &mut self.store,
            (
                retptr,
                challenge_ptr,
                challenge_len,
                prefix_ptr,
                prefix_len,
                difficulty,
            ),
        )?;

        // status: retptr 处 4 字节小端 i32（signed）
        // value:  retptr+8 处 8 字节小端 f64
        let data = self.memory.data(&mut self.store);
        let status = i32::from_le_bytes(
            data[retptr as usize..retptr as usize + 4]
                .try_into()
                .context("retptr out of memory bounds")?,
        );
        let answer = if status == 0 {
            None
        } else {
            let bytes: [u8; 8] = data[retptr as usize + 8..retptr as usize + 16]
                .try_into()
                .context("retptr+8 out of memory bounds")?;
            Some(f64::from_le_bytes(bytes) as i64)
        };

        // 恢复栈指针（对应 Python 的 finally 分支）
        self.add_to_stack_pointer(16)?;

        Ok(answer)
    }
}

/// PoW 求解器。`hasher` 惰性初始化并全局复用（`OnceLock`），
/// 对应 Python 版 `PoWSolver._init_hasher` 的惰性语义。
pub struct PoWSolver;

impl PoWSolver {
    fn hasher() -> &'static std::sync::Mutex<WasmHasher> {
        static HASHER: OnceLock<std::sync::Mutex<WasmHasher>> = OnceLock::new();
        HASHER.get_or_init(|| {
            std::sync::Mutex::new(WasmHasher::new().expect("failed to initialize WASM PoW hasher"))
        })
    }

    /// 求解 challenge，返回 base64 编码的 `x-ds-pow-response` 头值。
    /// 对应 Python `solve_challenge`。
    pub fn solve_challenge(config: &Challenge) -> Result<String> {
        let prefix = format!("{}_{}_", config.salt, config.expire_at);
        let mut hasher = Self::hasher()
            .lock()
            .map_err(|e| anyhow::anyhow!("pow hasher lock poisoned: {e}"))?;
        let answer = hasher.solve(&config.challenge, &prefix, config.difficulty)?;

        let result = serde_json::json!({
            "algorithm": config.algorithm,
            "challenge": config.challenge,
            "salt": config.salt,
            "answer": answer,
            "signature": config.signature,
            "target_path": config.target_path,
        });
        let encoded = serde_json::to_vec(&result).context("failed to serialize pow response")?;
        Ok(base64::engine::general_purpose::STANDARD.encode(encoded))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 用合成 challenge 验证 wasm_solve 调用机制：
    /// 加载 wasm → 写内存 → 调用 → 解析返回值，全程不依赖网络。
    ///
    /// 真实 challenge fixture 需在拿到 token 后从线上抓取并固化（任务 8.4）。
    #[test]
    fn solve_synthetic_challenge_is_deterministic() {
        let config = Challenge {
            algorithm: "SHA3-256".into(),
            challenge: "test-challenge-payload".into(),
            salt: "test-salt".into(),
            difficulty: 2.0,
            expire_at: 1_752_000_000,
            signature: "test-signature".into(),
            target_path: "/api/v0/chat/completion".into(),
        };

        // 确定性：同一 challenge 两次求解结果一致
        let first = PoWSolver::solve_challenge(&config).expect("solve should succeed");
        let second = PoWSolver::solve_challenge(&config).expect("solve should succeed");
        assert_eq!(first, second);

        // 输出是合法的 base64 JSON
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&first)
            .expect("output should be valid base64");
        let json: serde_json::Value =
            serde_json::from_slice(&decoded).expect("output should be valid JSON");
        assert_eq!(json["algorithm"], "SHA3-256");
        assert_eq!(json["challenge"], "test-challenge-payload");
        assert_eq!(json["target_path"], "/api/v0/chat/completion");
        // 合成 challenge 无法通过 wasm 内部的哈希校验，预期 status==0 → answer 为 null。
        // 这证明了 wasm 加载、内存写入、wasm_solve 调用与返回值解析全链路正确；
        // 真实 challenge 的答案验证见下方 `solve_real_challenge_fixture`（任务 8.4）。
        assert!(json["answer"].is_null());
    }

    /// 真实 challenge fixture 回归测试（任务 8.4）：
    /// 从线上 create_pow_challenge 抓取并固化的真实 challenge，验证 wasm 求解器
    /// 对真实 challenge（DeepSeekHashV1，difficulty=144000）能算出有效答案
    /// （status==1，answer 非 null）。全程离线，不依赖网络与 token。
    #[test]
    fn solve_real_challenge_fixture() {
        let config = Challenge {
            algorithm: "DeepSeekHashV1".into(),
            challenge: "7875b8299c8a754a2d400f2874575b51e587405c2662b4c4a12c63d7174772d4".into(),
            salt: "142efca7113322f2a9eb".into(),
            difficulty: 144000.0,
            expire_at: 1_786_261_326_652,
            signature: "b4d1c2d7a40ecbc9b496ac8aafa9e49db2edcd9d5c6c211f348d14c064712c1f".into(),
            target_path: "/api/v0/chat/completion".into(),
        };

        let result = PoWSolver::solve_challenge(&config).expect("solve should succeed");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(&result)
            .expect("output should be valid base64");
        let json: serde_json::Value =
            serde_json::from_slice(&decoded).expect("output should be valid JSON");
        // 真实 challenge 必须算出答案（answer 非 null），且算法/挑战值回显一致
        assert!(
            !json["answer"].is_null(),
            "real challenge must produce an answer"
        );
        assert_eq!(json["algorithm"], "DeepSeekHashV1");
        assert_eq!(
            json["challenge"],
            "7875b8299c8a754a2d400f2874575b51e587405c2662b4c4a12c63d7174772d4"
        );
        assert_eq!(json["target_path"], "/api/v0/chat/completion");
    }
}
