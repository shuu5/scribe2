//! 検出の起点（設計 docs/design/gate-cost.md §50 形 (1)(2)・ADR-0111・`s2-07l.736.36`）。
//!
//! 検出線を 1 日 1 回に間引く（日次の検出）ための小さな状態で、state dir の pipe の置き場の直下の dir `detection-origin` の中の
//! file `origin` が 1 行 `measured=<sha か -> fired=<epoch 秒>` を持つ。`measured` は前に測り終えた着地の sha（まだ無い印 `-`）、
//! `fired` は前に口を起こした時刻。直下の file にしないのは、置き場の直下の項目を便の id として列挙する読み手が在るため
//! （直下の file の項目は印を取れず着地の列の待ちを切る）。
//!
//! 読みは 2 語ちょうどのときだけ（[`parse_line`]・ほかの形は「読めない」の 1 値）。書きは同じ dir の一時 file へ 1 行を書いて
//! rename で置き換え、読み書きは store の lock の実装 1 本（[`locked`]・lock file は起点の名に `.lock`）の内で行う。
//! 本 file は event の種別・段・rules の kind の型を名指さない（閉包・設計 §50 形 (12)）。

use crate::fleet::store::{acquire, LockPolicy};
use crate::pipe::run_dir;
use crate::rules::int_row;
use crate::rules::manifest::Manifest;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 起こす間隔の下限（秒）を持つ rules 行の id（行を id の字で引く読み手はこの 1 本・値は code に焼かない・C5）。
const ROW_DAILY_MIN: &str = "detection.daily_min_s";

/// 起点を置く dir の名（pipe の置き場の直下・便の id と重ならない名）。
const ORIGIN_DIR: &str = "detection-origin";

/// 起点の file の名。
const ORIGIN_FILE: &str = "origin";

/// 起点を読めた 2 値。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Origin {
    /// 前に測り終えた着地の sha（まだ無い周は `None`＝file の字は `-`）。
    pub(super) measured: Option<String>,
    /// 前に口を起こした時刻（epoch 秒）。
    pub(super) fired: u64,
}

/// 下限（秒）を読む。行が無い・不発効・整数でない周は `Err`（間引かない今の形・既定値で埋めない）。
pub(super) fn daily_min_s(manifest: &Manifest) -> Result<u64, String> {
    int_row(manifest, ROW_DAILY_MIN)
}

/// 起点の 1 行を読む（空白で割った 2 語ちょうど・`measured=` が 16 進だけの 40 字か 64 字か `-`・`fired=` が 10 進）。
pub(super) fn parse_line(text: &str) -> Option<Origin> {
    let mut words = text.split_whitespace();
    let (first, second) = (words.next()?, words.next()?);
    if words.next().is_some() {
        return None;
    }
    let measured = match first.strip_prefix("measured=")? {
        "-" => None,
        sha if matches!(sha.len(), 40 | 64) && sha.chars().all(|c| c.is_ascii_hexdigit()) => Some(sha.to_owned()),
        _ => return None,
    };
    let digits = second.strip_prefix("fired=")?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(Origin { measured, fired: digits.parse().ok()? })
}

/// 起点の 1 行（改行つき・[`parse_line`] が読み返せる字）。
pub(super) fn compose_line(origin: &Origin) -> String {
    format!("measured={} fired={}\n", origin.measured.as_deref().unwrap_or("-"), origin.fired)
}

/// 口を起こすか（今・前に起こした時刻・下限を受ける pure な判定）。読めない起点（`None`）は起こす。
/// `fired + 下限` は飽和加算で、`fired` が上限の起点は起こさない（panic しない）。
pub(super) fn due(now: u64, fired: Option<u64>, floor: u64) -> bool {
    fired.is_none_or(|at| at.saturating_add(floor) <= now)
}

/// 測り終えた着地 `sha` へ起点を進めるときの次の値（動かさない周は `None`）。
///
/// 後ろへ戻さない: `sha` が今の `measured` と等しいか祖先（`behind(sha, measured)`）の周は動かさない。`fired` は読めた値の
/// まま（読めない起点は `now`）。
pub(super) fn advanced(
    current: Option<&Origin>,
    sha: &str,
    now: u64,
    behind: impl Fn(&str, &str) -> bool,
) -> Option<Origin> {
    let next = |fired| Some(Origin { measured: Some(sha.to_owned()), fired });
    let Some(origin) = current else {
        return next(now);
    };
    match origin.measured.as_deref() {
        Some(measured) if measured == sha || behind(sha, measured) => None,
        _ => next(origin.fired),
    }
}

/// 今の epoch 秒（時計を読めない周は 0＝下限を過ぎたと読まない側）。
pub(super) fn now_epoch() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |found| found.as_secs())
}

/// 起点の file の path。
fn origin_path(state_dir: &Path) -> PathBuf {
    run_dir(state_dir, ORIGIN_DIR).join(ORIGIN_FILE)
}

/// store の lock の内で `body`（起点の file の path を受ける）を撃つ。dir を作ってから lock を取り（作れない周は取れない周と
/// 同じ `Err`）、終えたら lock file を消す。待ち方は `policy`。
pub(super) fn locked<T>(state_dir: &Path, policy: LockPolicy, body: impl FnOnce(&Path) -> T) -> Result<T, String> {
    let file = origin_path(state_dir);
    let dir = file.parent().ok_or_else(|| format!("{} の dir が無い", file.display()))?;
    std::fs::create_dir_all(dir).map_err(|err| format!("{} を作れない: {err}", dir.display()))?;
    let lock = suffixed(&file, ".lock");
    acquire(&lock, policy).map_err(|err| format!("検出の起点の lock を取れない: {err}"))?;
    let out = body(&file);
    // 外せない lock は次の取り手が所有者の生死と stale の線で回収する（ここで止めない）。
    let _ = std::fs::remove_file(&lock);
    Ok(out)
}

/// 起点を読む（無い・読めない形は `None`＝片方だけを読む形は持たない）。lock の内で呼ぶ。
pub(super) fn read(file: &Path) -> Option<Origin> {
    parse_line(&std::fs::read_to_string(file).ok()?)
}

/// 起点を書く（同じ dir の一時 file へ 1 行を書いて rename で置き換える）。lock の内で呼ぶ。
pub(super) fn write(file: &Path, origin: &Origin) -> Result<(), String> {
    let partial = suffixed(file, ".partial");
    std::fs::write(&partial, compose_line(origin)).map_err(|err| format!("{} を書けない: {err}", partial.display()))?;
    std::fs::rename(&partial, file).map_err(|err| format!("{} を置き換えられない: {err}", file.display()))
}

/// path の名に接尾辞を足す。
fn suffixed(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}
