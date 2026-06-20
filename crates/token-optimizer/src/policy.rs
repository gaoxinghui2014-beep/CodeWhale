//! 压缩策略模块 —— 缓存感知的压缩决策。
//!
//! 参考 headroom 的 `compression_policy.rs`，提供 `net_mutation_gain`
//! 公式来评估修改缓存内容的净收益。

/// 缓存写成本乘数（Anthropic: 1.25× 普通输入 token）。
pub const CACHE_WRITE_MULTIPLIER: f64 = 1.25;

/// 缓存读成本乘数（Anthropic: 0.1× 普通输入 token）。
pub const CACHE_READ_MULTIPLIER: f64 = 0.10;

/// 压缩策略 —— 决定何时压缩以及压缩的激进程度。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CompressStrategy {
    /// 保守 —— 优先保证缓存稳定性。
    Conservative,
    /// 平衡 —— 兼顾缓存和压缩。
    Balanced,
    /// 激进 —— 优先压缩，即使可能破坏缓存。
    Aggressive,
}

impl CompressStrategy {
    /// 最大有损压缩比例（上限）。
    pub fn max_lossy_ratio(&self) -> f64 {
        match self {
            Self::Conservative => 0.25,
            Self::Balanced => 0.45,
            Self::Aggressive => 0.65,
        }
    }

    /// 易变内容阈值（token 数），低于此阈值的变动视为缓存稳定。
    pub fn volatile_token_threshold(&self) -> u32 {
        match self {
            Self::Conservative => 32,
            Self::Balanced => 128,
            Self::Aggressive => 256,
        }
    }

    /// 是否启用 live-zone-only 模式（不修改缓存前缀）。
    pub fn live_zone_only(&self) -> bool {
        matches!(self, Self::Conservative)
    }
}

/// 计算修改缓存内容的净收益（以普通输入 token 成本为单位）。
///
/// 参考 headroom 的 `net_mutation_gain` 公式：
///
/// ```text
/// gain = ΔT · (w + r·(R − 1))  −  P_alive · (w − r) · (S + ΔT)
/// ```
///
/// # 参数
///
/// - `delta_t`: 要删除的 token 数
/// - `suffix_tokens`: 编辑点之后的缓存 token 数
/// - `expected_reads`: 预期剩余读取次数
/// - `p_alive`: 缓存存活的概率（0.0–1.0）
///
/// # 返回
///
/// 净收益（正值 = 压缩划算，负值 = 破坏缓存损失更大）
pub fn net_mutation_gain(
    delta_t: u32,
    suffix_tokens: u32,
    expected_reads: f64,
    p_alive: f64,
) -> f64 {
    let w = CACHE_WRITE_MULTIPLIER;
    let r = CACHE_READ_MULTIPLIER;

    let delta = delta_t as f64;
    let suffix = suffix_tokens as f64;
    let reads = expected_reads.max(0.0);
    let alive = p_alive.clamp(0.0, 1.0);

    // 第一项：节省的读写成本
    let saved = delta * (w + r * (reads - 1.0));

    // 第二项：缓存失效的惩罚
    let penalty = alive * (w - r) * (suffix + delta);

    saved - penalty
}

/// 判断是否应该修改深层缓存中的内容。
///
/// 当 `net_mutation_gain > 0` 时返回 true。
pub fn should_mutate_deep(
    delta_t: u32,
    suffix_tokens: u32,
    expected_reads: f64,
    p_alive: f64,
) -> bool {
    net_mutation_gain(delta_t, suffix_tokens, expected_reads, p_alive) > 0.0
}

/// 计算盈亏平衡所需的最小读取次数。
///
/// ```text
/// R = ((w - r) / r) · (S / ΔT)
/// ```
pub fn break_even_reads(delta_t: u32, suffix_tokens: u32) -> f64 {
    if delta_t == 0 {
        return 0.0;
    }

    let w = CACHE_WRITE_MULTIPLIER;
    let r = CACHE_READ_MULTIPLIER;

    ((w - r) / r) * (suffix_tokens as f64 / delta_t as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cold_cache_always_profitable() {
        // 缓存已死（P_alive=0），任何压缩都盈利
        assert!(should_mutate_deep(2000, 50000, 0.0, 0.0));
    }

    #[test]
    fn small_shave_deep_suffix_not_profitable() {
        // 小规模裁剪 + 深层缓存后缀 → 不盈利
        // 2K shave under 50K warm suffix: gain ≈ -55500
        let gain = net_mutation_gain(2000, 50000, 10.0, 1.0);
        assert!(gain < 0.0);
        assert!(!should_mutate_deep(2000, 50000, 10.0, 1.0));
    }

    #[test]
    fn big_shave_shallow_suffix_profitable() {
        // 大规模裁剪 + 浅层缓存后缀 → 盈利
        // 50K shave under 10K suffix: gain ≈ 3500
        let gain = net_mutation_gain(50000, 10000, 3.0, 1.0);
        assert!(gain > 0.0);
        assert!(should_mutate_deep(50000, 10000, 3.0, 1.0));
    }

    #[test]
    fn no_suffix_always_profitable() {
        // 编辑缓存边界处（S=0），只要还有读取就盈利
        assert!(should_mutate_deep(1, 0, 1.0, 1.0));
    }

    #[test]
    fn break_even_reads_anchors() {
        // 2K shave under 50K suffix: R = 11.5 * 25 = 287.5 (rarely profitable)
        let r = break_even_reads(2000, 50000);
        assert!((r - 287.5).abs() < 1.0);

        // 50K shave under 10K suffix: R = 11.5 * 0.2 = 2.3 (profitable)
        let r = break_even_reads(50000, 10000);
        assert!((r - 2.3).abs() < 0.05);
    }

    #[test]
    fn strategy_max_lossy_ratio() {
        assert_eq!(CompressStrategy::Conservative.max_lossy_ratio(), 0.25);
        assert_eq!(CompressStrategy::Balanced.max_lossy_ratio(), 0.45);
        assert_eq!(CompressStrategy::Aggressive.max_lossy_ratio(), 0.65);
    }

    #[test]
    fn zero_delta_t_gives_zero_break_even() {
        assert_eq!(break_even_reads(0, 1000), 0.0);
    }
}
