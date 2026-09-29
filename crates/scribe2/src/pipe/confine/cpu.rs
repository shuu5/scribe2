//! 便の箱の CPU の上限（`CPUQuota`）の導出（設計 docs/design/gate-cost.md §45 形 4・ADR-0095）。
//!
//! 幅は**受付が配った幅**（`jobs × threads`）で、配らない箱（受付を通らない行・runner・lens・claude の包み）は
//! **1 job の値段**（[`Cpu::priced`] の thread 数）である。新しい rules 行は足さない（値段は `gate.mutants_jobs` と
//! 実測の core 数から出る）。core 数を読めず幅も無い周は上限を置かない（止めない・重みと memory だけの形）。

use super::Caps;
use crate::pipe::admission::Cpu;
use crate::pipe::health;

/// 1 thread が使う CPU の % （`CPUQuota=100%` = core 1 本）。
const PERCENT_PER_THREAD: u64 = 100;

/// 便の箱の上限（%）を導く（pure）。`width` が在れば `width × 100`（0 は 1 に切り上げる＝`CPUQuota=0%` を作らない）、
/// 無ければ 1 job の値段 × 100。幅が無く core 数を読めない周は `None`＝`CPUQuota` の 2 語を置かない。
pub(super) fn quota_percent(width: Option<u64>, cores: Option<u64>, mutants_jobs: u64) -> Option<u64> {
    let threads = match width {
        Some(width) => width.max(1),
        None => Cpu::priced(cores?, mutants_jobs).price,
    };
    Some(threads.saturating_mul(PERCENT_PER_THREAD))
}

/// 便の箱の上限（%）の読み口: host の core 数（[`health::host_cores`] の 1 本）と埋め込みの `gate.mutants_jobs` から
/// [`quota_percent`] へ渡す。
pub(super) fn quota_of(width: Option<u64>, caps: &Caps) -> Option<u64> {
    quota_percent(width, health::host_cores(), caps.mutants_jobs)
}

/// 席の箱の上限（%）の読み口: 埋め込みの [`Caps`] から [`quota_of`] へ幅 `None`（1 job の値段）を渡す。`Caps` か core 数を
/// 読めない周は `None`＝`CPUQuota` の 2 語を置かない（起動は止めない）。
pub fn seat_quota() -> Option<u64> {
    quota_of(None, &Caps::embedded().ok()?)
}

#[cfg(test)]
mod tests {
    use super::super::{scope_args, seat_scope_head};
    use super::quota_percent;

    /// (b) `scope_args` の 4 形（重み・上限の有無の組）の語列の全文と順（`MemoryMax` → `CPUWeight` → `CPUQuota` →
    /// `OOMPolicy`）。両方 `None` の形は席の頭の語列と等しい。
    #[test]
    fn cpu_width_scope_args_four_forms_in_order() {
        let head = ["--user", "--scope", "--quiet", "--collect", "--unit=u1", "-p", "MemoryMax=4096M"];
        let weight = ["-p", "CPUWeight=50"];
        let quota = ["-p", "CPUQuota=800%"];
        let tail = ["-p", "OOMPolicy=continue"];
        let form = |with: &[&[&str]]| -> Vec<String> {
            head.iter().chain(with.iter().flat_map(|words| words.iter())).chain(tail.iter()).map(|word| (*word).to_owned()).collect()
        };
        assert_eq!(scope_args("u1", 4096, Some(50), Some(800)), form(&[&weight, &quota]), "重みと上限");
        assert_eq!(scope_args("u1", 4096, Some(50), None), form(&[&weight]), "重みだけ");
        assert_eq!(scope_args("u1", 4096, None, Some(800)), form(&[&quota]), "上限だけ");
        assert_eq!(scope_args("u1", 4096, None, None), form(&[]), "どちらも無い");
        let seat = seat_scope_head("u1", 4096, None);
        assert_eq!(seat.get(1..seat.len().saturating_sub(1)), Some(&form(&[])[..]), "席の頭は両方 None の形: {seat:?}");
    }

    /// (c) 便の箱の上限の導出: 幅が在れば幅 × 100%・無ければ 1 job の値段 × 100%（値段の床は 1）・core 数を読めない周は語が無い。
    #[test]
    fn cpu_width_quota_follows_the_width_else_the_one_job_price() {
        assert_eq!(quota_percent(Some(12), Some(32), 4), Some(1200), "受付が配った幅");
        assert_eq!(quota_percent(Some(1), Some(32), 4), Some(100), "縮退と測れない周の幅 1");
        assert_eq!(quota_percent(Some(12), None, 4), Some(1200), "幅が在れば core 数は読まない");
        assert_eq!(quota_percent(Some(0), Some(32), 4), Some(100), "幅 0 は 100% に切り上げる（0% を作らない）");
        assert_eq!(quota_percent(None, Some(32), 4), Some(800), "32 core / gate.mutants_jobs 4 = 8 thread");
        assert_eq!(quota_percent(None, Some(3), 4), Some(100), "値段の床は 1");
        assert_eq!(quota_percent(None, Some(4), 4), Some(100), "4 core / 4 = 1");
        assert_eq!(quota_percent(None, Some(16), 0), Some(1600), "gate.mutants_jobs 0 は 1 で割る");
        assert_eq!(quota_percent(None, None, 4), None, "core 数を読めない周は上限を置かない");
    }
}
