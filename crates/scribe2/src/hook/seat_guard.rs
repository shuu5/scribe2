//! 席の context が上限を超えた周の編集を止める（設計 docs/design/seat-autonomy.md §3・
//! SRS FR26 / FR21 / FR23・憲法 C10 / C11 / C2.2 / CON5）。
//!
//! 止めるのは**編集の時点**である。席が上限を超えたまま書き続けると、退避する余地ごと
//! 失って cycle が回らなくなる。ただし**退避そのものは通す**（[`SeatDecision::Externalize`]）
//! ——止めると席は退避すらできず、上限を超えたまま身動きが取れない。
//!
//! **測れない周は deny しない**（FR26）。C16 の fail-closed は「測れたのに通す」を禁じる
//! のであって、測れない周に止めることを求めてはいない。ここで deny へ倒すと transcript を
//! 持たない周の編集が全部止まる＝器が自分の席を使えなくする。代わりに**記録を 1 行残す**
//! （FR21）ので、測れていない周が続いていることは後から数えられる。
//!
//! **env も HOME も tmux も見ない**（C2.2・契約の「やらない」）。読むのは payload が
//! 名指した transcript と、binary に埋め込んだ rules manifest だけである。

use super::guard::GUARDED;
use crate::name::NAME;
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
use crate::seat::meter;
use std::path::{Component, Path, PathBuf};

/// 退避物が置かれる dir（`<root>/.claude-session/`）。
const SESSION_DIR: &str = ".claude-session";
/// 退避物の名前の前置き（FR23 の退避物の名前）。
const WM_PREFIX: &str = "working-memory.";
/// 退避物の名前の後置き。
const WM_SUFFIX: &str = ".md";
/// cap を宣言する rules 行の id（**値は code に焼かない**）。
const ID_CAP: &str = "seat.context_cap_pct";
/// 窓を宣言する rules 行の id（**値は code に焼かない**）。
const ID_WINDOW: &str = "seat.context_window_tokens";
/// payload が transcript を名指していない周の理由。
const NO_TRANSCRIPT: &str = "no-transcript-path";
/// 宣言（rules 行）が読めない周の理由。
const NO_RULE: &str = "no-rule";
/// 百分率の分子。
const PERCENT: u64 = 100;

/// seat guard の判定。**bool で持たない**（憲法 C11）。
///
/// 「通す」が 2 つ在るのは、通した理由が別物だからである——[`Self::Allow`] は「測ったうえで
/// 上限未満」、[`Self::Externalize`] は「上限に関わらず通す口」。1 つに畳むと、退避の口が
/// 効いているのか単に上限未満なのかを歯からも記録からも読めなくなる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeatDecision {
    /// 上限未満＝通す（0 byte・記録なし）。
    Allow,
    /// 退避の口＝使用率に関わらず通す。
    Externalize,
    /// 上限以上＝止める。中身は stderr へ出す 1 行。
    Deny(String),
    /// 測れない＝**止めない**。中身は記録に残す理由の 1 語。
    Unmeasured(String),
}

/// 1 回の編集について判定する。
///
/// 対象 tool は write-set guard と**同じ集合**（[`GUARDED`]）を参照する＝`Bash` は見ない。
/// 集合を写して持つと 2 面がドリフトし、片方だけが効く窓ができる。
pub fn decide(
    root: &Path,
    cwd: &Path,
    tool: &str,
    path: Option<&str>,
    transcript: Option<&str>,
) -> SeatDecision {
    if !GUARDED.contains(&tool) {
        return SeatDecision::Allow;
    }
    if is_externalize(root, cwd, path) {
        return SeatDecision::Externalize;
    }
    let Some((cap, window)) = thresholds() else {
        return SeatDecision::Unmeasured(NO_RULE.to_owned());
    };
    let Some(found) = transcript else {
        return SeatDecision::Unmeasured(NO_TRANSCRIPT.to_owned());
    };
    let used = match meter::used_from_transcript(Path::new(found)) {
        Ok(found) => found,
        Err(reason) => return SeatDecision::Unmeasured(reason.to_owned()),
    };
    // 切り捨て。境界は「cap 以上で止める」ので、切り上げると cap 未満の周まで止まる。
    let pct = used.saturating_mul(PERCENT) / window;
    if pct >= cap {
        SeatDecision::Deny(format!(
            "{NAME}: deny context {pct}% ≥ cap {cap}%（退避してから・FR26）"
        ))
    } else {
        SeatDecision::Allow
    }
}

/// cap と窓を manifest から読む。**どちらか欠けたら測らない**（0 で割らない）。
fn thresholds() -> Option<(u64, u64)> {
    let manifest = Manifest::embedded().ok()?;
    let cap = int_of(&manifest, ID_CAP)?;
    let window = int_of(&manifest, ID_WINDOW)?;
    (window > 0).then_some((cap, window))
}

/// 発効している行の整数値。不発効・別の形は `None`（＝測らない側へ倒す）。
fn int_of(manifest: &Manifest, id: &str) -> Option<u64> {
    let row = manifest.get(id)?;
    match (row.enabled, &row.value) {
        (true, RuleValue::Int(found)) => Some(*found),
        _ => None,
    }
}

/// 編集先が退避の口か（`<root>/.claude-session/working-memory.*.md` **ちょうど**）。
///
/// 判定は**狭く**取る。この口は上限を無視して通す側なので、広く取ると「退避のふりをした
/// 編集」が上限を素通りする。段数は 2 段ちょうどで、`..` や `.` を含む形は
/// [`Component::Normal`] の照合から外れて落ちる。
fn is_externalize(root: &Path, cwd: &Path, path: Option<&str>) -> bool {
    let Some(target) = path else {
        return false;
    };
    let raw = Path::new(target);
    let absolute = if raw.is_absolute() {
        PathBuf::from(raw)
    } else {
        cwd.join(raw)
    };
    let Ok(rel) = absolute.strip_prefix(root) else {
        return false;
    };
    let parts: Vec<Component> = rel.components().collect();
    let [Component::Normal(dir), Component::Normal(file)] = parts.as_slice() else {
        return false;
    };
    if dir.to_str() != Some(SESSION_DIR) {
        return false;
    }
    let Some(name) = file.to_str() else {
        return false;
    };
    // 前置きと後置きが**重なる**字面（`working-memory.md`）は退避物の名前ではない。
    name.len() >= WM_PREFIX.len() + WM_SUFFIX.len()
        && name.starts_with(WM_PREFIX)
        && name.ends_with(WM_SUFFIX)
}
