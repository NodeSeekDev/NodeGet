//! JS 运行时核心类型定义。
//!
//! 包含 `RunType`、`CompileMode`、`JsCodeInput` 以及运行时池状态类型，
//! 默认 feature 下即可使用，不依赖 server 专有逻辑。

use serde::{Deserialize, Serialize};

/// JS Worker 的运行模式，决定调用哪个 handler 函数。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunType {
    /// 手动调用，对应 `export default { onCall() }`
    Call,
    /// 定时任务调用，对应 `export default { onCron() }`
    Cron,
    /// HTTP 路由调用，对应 `export default { onRoute() }`
    Route,
    /// 内联调用（从另一个 JS Worker 中调用），对应 `export default { onInlineCall() }`
    InlineCall,
}

/// JS 脚本编译模式。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompileMode {
    /// 使用预编译字节码执行（默认，性能更优）
    #[default]
    Bytecode,
    /// 使用源码模式执行（每次重新解析编译）
    Source,
}

impl RunType {
    /// 返回运行模式的字符串标识，用于序列化和日志。
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Cron => "cron",
            Self::Route => "route",
            Self::InlineCall => "inline_call",
        }
    }

    /// 返回 JS 端对应的 handler 函数名（如 `"onCall"`）。
    #[must_use]
    pub const fn handler_name(&self) -> &'static str {
        match self {
            Self::Call => "onCall",
            Self::Cron => "onCron",
            Self::Route => "onRoute",
            Self::InlineCall => "onInlineCall",
        }
    }
}

/// JS 代码输入形式，区分源码和预编译字节码。
#[derive(Debug, Clone)]
pub enum JsCodeInput {
    /// JS 源码字符串
    Source(String),
    /// `QuickJS` 预编译字节码
    Bytecode(Vec<u8>),
}

/// 单个运行时 Worker 的状态信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePoolWorkerInfo {
    /// Worker 对应的脚本名称
    pub script_name: String,
    /// 当前正在执行的请求数
    pub active_requests: usize,
    /// 上次使用时间（ms，unix epoch）
    pub last_used_ms: i64,
    /// 空闲时长（ms）
    pub idle_ms: i64,
    /// 运行时清理时间阈值（ms），None 表示永不清理
    pub runtime_clean_time_ms: Option<i64>,
}

/// 运行时池整体状态信息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePoolInfo {
    /// Worker 总数
    pub total_workers: usize,
    /// 各 Worker 的详细状态列表
    pub workers: Vec<RuntimePoolWorkerInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_type_as_str() {
        assert_eq!(RunType::Call.as_str(), "call");
        assert_eq!(RunType::Cron.as_str(), "cron");
        assert_eq!(RunType::Route.as_str(), "route");
        assert_eq!(RunType::InlineCall.as_str(), "inline_call");
    }

    #[test]
    fn run_type_handler_name() {
        assert_eq!(RunType::Call.handler_name(), "onCall");
        assert_eq!(RunType::Cron.handler_name(), "onCron");
        assert_eq!(RunType::Route.handler_name(), "onRoute");
        assert_eq!(RunType::InlineCall.handler_name(), "onInlineCall");
    }

    #[test]
    fn run_type_serde_roundtrip() {
        let variants = [
            (RunType::Call, "call"),
            (RunType::Cron, "cron"),
            (RunType::Route, "route"),
            (RunType::InlineCall, "inline_call"),
        ];
        for (rt, expected_name) in variants {
            let json = serde_json::to_string(&rt).unwrap();
            let parsed: RunType = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed.as_str(), expected_name);
        }
    }

    #[test]
    fn run_type_serde_snake_case() {
        let json = serde_json::to_string(&RunType::InlineCall).unwrap();
        assert!(json.contains("inline_call"));
        let json = serde_json::to_string(&RunType::Call).unwrap();
        assert!(json.contains("call"));
    }

    #[test]
    fn compile_mode_default_is_bytecode() {
        assert!(matches!(CompileMode::default(), CompileMode::Bytecode));
    }

    #[test]
    fn compile_mode_serde_roundtrip() {
        let bytecode_json = serde_json::to_string(&CompileMode::Bytecode).unwrap();
        assert!(bytecode_json.contains("bytecode"));
        let parsed: CompileMode = serde_json::from_str(&bytecode_json).unwrap();
        assert!(matches!(parsed, CompileMode::Bytecode));

        let source_json = serde_json::to_string(&CompileMode::Source).unwrap();
        assert!(source_json.contains("source"));
        let parsed: CompileMode = serde_json::from_str(&source_json).unwrap();
        assert!(matches!(parsed, CompileMode::Source));
    }

    #[test]
    fn compile_mode_serde_snake_case() {
        let json = serde_json::to_string(&CompileMode::Bytecode).unwrap();
        assert!(json.contains("bytecode"));
        let json = serde_json::to_string(&CompileMode::Source).unwrap();
        assert!(json.contains("source"));
    }

    #[test]
    fn compile_mode_bytecode_deserialize() {
        let parsed: CompileMode = serde_json::from_str("\"bytecode\"").unwrap();
        assert!(matches!(parsed, CompileMode::Bytecode));
    }

    #[test]
    fn compile_mode_source_deserialize() {
        let parsed: CompileMode = serde_json::from_str("\"source\"").unwrap();
        assert!(matches!(parsed, CompileMode::Source));
    }

    #[test]
    fn js_code_input_source_variant() {
        let input = JsCodeInput::Source("console.log(1)".to_owned());
        assert!(matches!(input, JsCodeInput::Source(_)));
    }

    #[test]
    fn js_code_input_bytecode_variant() {
        let input = JsCodeInput::Bytecode(vec![0x01, 0x02, 0x03]);
        assert!(matches!(input, JsCodeInput::Bytecode(_)));
    }

    #[test]
    fn runtime_pool_worker_info_serde_roundtrip() {
        let info = RuntimePoolWorkerInfo {
            script_name: "test_script".to_owned(),
            active_requests: 3,
            last_used_ms: 1_700_000_000,
            idle_ms: 5000,
            runtime_clean_time_ms: Some(60000),
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: RuntimePoolWorkerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info.script_name, parsed.script_name);
        assert_eq!(info.active_requests, parsed.active_requests);
        assert_eq!(info.last_used_ms, parsed.last_used_ms);
        assert_eq!(info.idle_ms, parsed.idle_ms);
        assert_eq!(info.runtime_clean_time_ms, parsed.runtime_clean_time_ms);
    }

    #[test]
    fn runtime_pool_worker_info_none_clean_time() {
        let info = RuntimePoolWorkerInfo {
            script_name: "test".to_owned(),
            active_requests: 0,
            last_used_ms: 0,
            idle_ms: 0,
            runtime_clean_time_ms: None,
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: RuntimePoolWorkerInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.runtime_clean_time_ms, None);
    }

    #[test]
    fn runtime_pool_info_serde_roundtrip() {
        let info = RuntimePoolInfo {
            total_workers: 2,
            workers: vec![
                RuntimePoolWorkerInfo {
                    script_name: "a".to_owned(),
                    active_requests: 1,
                    last_used_ms: 100,
                    idle_ms: 10,
                    runtime_clean_time_ms: None,
                },
                RuntimePoolWorkerInfo {
                    script_name: "b".to_owned(),
                    active_requests: 0,
                    last_used_ms: 200,
                    idle_ms: 50,
                    runtime_clean_time_ms: Some(30000),
                },
            ],
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: RuntimePoolInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(info.total_workers, parsed.total_workers);
        assert_eq!(info.workers.len(), parsed.workers.len());
    }

    #[test]
    fn runtime_pool_info_empty_workers() {
        let info = RuntimePoolInfo {
            total_workers: 0,
            workers: vec![],
        };
        let json = serde_json::to_string(&info).unwrap();
        let parsed: RuntimePoolInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.total_workers, 0);
        assert!(parsed.workers.is_empty());
    }
}
