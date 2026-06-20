//! 上下文窗口卫士与熔断器机制。
//!
//! 参考 openhuman-main 的 ContextGuard 设计，在每次 LLM 调用前检查
//! 上下文利用率，并在连续压缩失败后熔断以防止无限重试循环。
//!
//! # 阈值
//!
//! - **软阈值 (90%)**: 触发压缩（microcompact 或 autocompact）。
//! - **硬阈值 (95%)**: 如果熔断器已跳闸，拒绝调用。
//! - **熔断**: 连续 3 次压缩失败后跳闸。
#![allow(dead_code)]

/// 软阈值（0.0–1.0），超过此值触发压缩。
pub const COMPACTION_TRIGGER_THRESHOLD: f64 = 0.90;

/// 硬阈值（0.0–1.0），超过此值且熔断器跳闸时拒绝调用。
const HARD_LIMIT_THRESHOLD: f64 = 0.95;

/// 连续压缩失败次数达到此值后熔断器跳闸。
const MAX_CONSECUTIVE_FAILURES: u8 = 3;

/// 预调用上下文检查的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextCheckResult {
    /// 上下文利用率在安全范围内。
    Ok,
    /// 上下文接近容量上限，应尝试压缩。
    CompactionNeeded,
    /// 上下文严重超限且压缩已被熔断器禁用。
    ContextExhausted {
        utilization_pct: u8,
        reason: String,
    },
}

/// 跟踪上下文窗口利用率与压缩健康状态。
///
/// 每次 LLM 调用后通过 [`Self::update_usage`] 更新 token 使用量。
/// 调用前通过 [`Self::check`] 判断是否需要压缩。
/// 压缩成功/失败通过 [`Self::record_compaction_success`] /
/// [`Self::record_compaction_failure`] 反馈给熔断器。
#[derive(Debug)]
pub struct ContextGuard {
    /// 上次已知的输入 token 数。
    last_input_tokens: u64,
    /// 上次已知的输出 token 数。
    last_output_tokens: u64,
    /// 模型上下文窗口大小（0 = 未知，卫士为 no-op）。
    context_window: u64,
    /// 连续压缩失败次数。
    consecutive_compaction_failures: u8,
    /// 压缩是否已被熔断器禁用。
    compaction_disabled: bool,
}

impl Default for ContextGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextGuard {
    pub fn new() -> Self {
        Self {
            last_input_tokens: 0,
            last_output_tokens: 0,
            context_window: 0,
            consecutive_compaction_failures: 0,
            compaction_disabled: false,
        }
    }

    /// 使用已知的上下文窗口大小创建卫士。
    pub fn with_context_window(context_window: u64) -> Self {
        Self {
            context_window,
            ..Self::new()
        }
    }

    /// 使用最新的 provider 使用信息更新卫士。
    pub fn update_usage(&mut self, input_tokens: u64, output_tokens: u64, context_window: u64) {
        self.last_input_tokens = input_tokens;
        self.last_output_tokens = output_tokens;
        if context_window > 0 {
            self.context_window = context_window;
        }
    }

    /// 估算当前上下文利用率（0.0–1.0）。
    /// 上下文窗口未知时返回 `None`。
    pub fn utilization(&self) -> Option<f64> {
        if self.context_window == 0 {
            return None;
        }
        let total_used = self.last_input_tokens + self.last_output_tokens;
        Some(total_used as f64 / self.context_window as f64)
    }

    /// 检查上下文是否可以安全进行下一次 LLM 调用。
    pub fn check(&self) -> ContextCheckResult {
        let utilization = match self.utilization() {
            Some(u) => u,
            None => return ContextCheckResult::Ok, // 未知窗口 = 不拦截
        };

        if utilization >= HARD_LIMIT_THRESHOLD && self.compaction_disabled {
            return ContextCheckResult::ContextExhausted {
                utilization_pct: (utilization * 100.0) as u8,
                reason: format!(
                    "上下文已使用 {:.0}%；压缩已熔断（连续 {} 次失败）",
                    utilization * 100.0,
                    self.consecutive_compaction_failures
                ),
            };
        }

        if utilization >= COMPACTION_TRIGGER_THRESHOLD && !self.compaction_disabled {
            return ContextCheckResult::CompactionNeeded;
        }

        ContextCheckResult::Ok
    }

    /// 记录一次成功的压缩，重置失败计数器。
    pub fn record_compaction_success(&mut self) {
        self.consecutive_compaction_failures = 0;
        self.compaction_disabled = false;
        tracing::debug!("[context_guard] 压缩成功，熔断器已重置");
    }

    /// 记录一次失败的压缩尝试。
    /// 连续失败 [`MAX_CONSECUTIVE_FAILURES`] 次后跳闸熔断器。
    pub fn record_compaction_failure(&mut self) {
        self.consecutive_compaction_failures += 1;
        if self.consecutive_compaction_failures >= MAX_CONSECUTIVE_FAILURES {
            self.compaction_disabled = true;
            tracing::warn!(
                consecutive_failures = self.consecutive_compaction_failures,
                "[context_guard] 熔断器跳闸 —— 压缩已禁用"
            );
        } else {
            tracing::debug!(
                consecutive_failures = self.consecutive_compaction_failures,
                max = MAX_CONSECUTIVE_FAILURES,
                "[context_guard] 压缩失败，熔断器待命中"
            );
        }
    }

    /// 压缩熔断器当前是否已跳闸。
    pub fn is_compaction_disabled(&self) -> bool {
        self.compaction_disabled
    }

    /// 连续压缩失败次数。
    pub fn consecutive_failures(&self) -> u8 {
        self.consecutive_compaction_failures
    }

    /// 上次输入 token 数。
    pub fn last_input_tokens(&self) -> u64 {
        self.last_input_tokens
    }

    /// 上次输出 token 数。
    pub fn last_output_tokens(&self) -> u64 {
        self.last_output_tokens
    }

    /// 当前已知的模型上下文窗口。
    /// `0` 表示未知 —— 此时卫士为 no-op。
    pub fn context_window(&self) -> u64 {
        self.context_window
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_context_window_always_ok() {
        let guard = ContextGuard::new();
        assert_eq!(guard.check(), ContextCheckResult::Ok);
    }

    #[test]
    fn low_utilization_is_ok() {
        let mut guard = ContextGuard::with_context_window(100_000);
        guard.update_usage(10_000, 5_000, 100_000);
        assert_eq!(guard.check(), ContextCheckResult::Ok);
    }

    #[test]
    fn high_utilization_triggers_compaction() {
        let mut guard = ContextGuard::with_context_window(100_000);
        guard.update_usage(85_000, 6_000, 100_000);
        assert_eq!(guard.check(), ContextCheckResult::CompactionNeeded);
    }

    #[test]
    fn circuit_breaker_trips_after_consecutive_failures() {
        let mut guard = ContextGuard::with_context_window(100_000);
        guard.update_usage(90_000, 6_000, 100_000);
        // 连续 3 次失败
        guard.record_compaction_failure();
        guard.record_compaction_failure();
        assert!(!guard.is_compaction_disabled());
        guard.record_compaction_failure();
        assert!(guard.is_compaction_disabled());
        // 此后应该返回 ContextExhausted
        assert!(matches!(
            guard.check(),
            ContextCheckResult::ContextExhausted { .. }
        ));
    }

    #[test]
    fn success_resets_circuit_breaker() {
        let mut guard = ContextGuard::with_context_window(100_000);
        guard.update_usage(90_000, 6_000, 100_000);
        guard.record_compaction_failure();
        guard.record_compaction_failure();
        guard.record_compaction_success();
        assert!(!guard.is_compaction_disabled());
        assert_eq!(guard.consecutive_failures(), 0);
        assert_eq!(guard.check(), ContextCheckResult::CompactionNeeded);
    }

    #[test]
    fn utilization_calculation() {
        let mut guard = ContextGuard::with_context_window(200_000);
        guard.update_usage(100_000, 50_000, 200_000);
        let u = guard.utilization().unwrap();
        assert!((u - 0.75).abs() < 0.01);
    }
}
