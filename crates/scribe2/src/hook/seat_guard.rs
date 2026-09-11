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
/// payload が transcript を名指していない周の理由。
const NO_TRANSCRIPT: &str = "no-transcript-path";


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
    let Some(cap) = declared_cap() else {
        return SeatDecision::Unmeasured(meter::REASON_NO_RULE.to_owned());
    };
    // **空文字は「無い」と同じ**（trim 後）。空の口をそのまま path として扱うと、渡し忘れが
    // `unreadable`（file が壊れている）に化けて、記録から原因を取り違える。
    let Some(found) = transcript.map(str::trim).filter(|found| !found.is_empty()) else {
        return SeatDecision::Unmeasured(NO_TRANSCRIPT.to_owned());
    };
    // **使用率は自分で計算しない**——meter の 1 本の口を通す。2 面が別々に割ると、
    // 丸めや窓の出所が片方だけ動いたときに静かにずれる（本便が畳んだ穴そのもの）。
    let pct = match meter::used_from_transcript_pct(Path::new(found)) {
        Ok((pct, _, _)) => pct,
        Err(reason) => return SeatDecision::Unmeasured(reason.to_owned()),
    };
    if pct >= cap {
        SeatDecision::Deny(format!(
            "{NAME}: deny context {pct}% ≥ cap {cap}%（退避してから・FR26）"
        ))
    } else {
        SeatDecision::Allow
    }
}

/// cap を manifest から読む。**窓はここでは読まない**——窓で割るのは meter の口の仕事で、
/// 2 か所で読むと片方だけが別の行を見に行ける（同じ値を 2 面が持つ形にしない）。
fn declared_cap() -> Option<u64> {
    let manifest = Manifest::embedded().ok()?;
    int_of(&manifest, ID_CAP)
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
    at_mouth(root, &absolute) && !escapes_by_link(root, &absolute)
}

/// 字句の段。`<root>/.claude-session/working-memory.*.md` **ちょうど 2 段**か。
fn at_mouth(root: &Path, absolute: &Path) -> bool {
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

/// 実体の段。字句で口に見える file が、**link で口の外を指していないか**。
///
/// 通す側の口を字句 1 段で持つと、`working-memory.x.md` という名前の symlink を
/// 1 本張るだけで口の外の file を上限越しに書ける（実測 2026-09-10・lens-383 F4:
/// `src/lib.rs` への link が 65% で rc 0 になった）。兄弟の write-set guard が
/// 同じ罠に実体の段で備えているのと同じ理由である。
///
/// **まだ無い file は link ではありえない**ので、在るときだけ実体で確かめる
/// ——退避物は「これから作る」周がふつうで、そこで解けないことを理由に口を閉じると
/// 退避そのものが止まる。解けない link（dangling）は口に入れない＝通す側は fail-closed。
fn escapes_by_link(root: &Path, absolute: &Path) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(absolute) else {
        return false;
    };
    if !meta.file_type().is_symlink() {
        return false;
    }
    match (absolute.canonicalize(), root.canonicalize()) {
        (Ok(real), Ok(base)) => !at_mouth(&base, &real),
        _ => true,
    }
}
