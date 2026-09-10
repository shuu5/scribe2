//! context 使用量の計測（SRS FR25・設計 §3）。
//!
//! 出所は **pane（statusline）が一次・transcript（jsonl）が fallback** で、成立した
//! 出所を `source=` として必ず一緒に運ぶ（憲法 C10「Measured は出所付き」）。端末描画は
//! 出所の 1 つであって seat state の判定入力ではない（C3.3）。
//!
//! **計測できない周は 0% に化けない**（FR25）: 健全性を外れた statusline は捏造値を
//! 流さず不成立にし、理由を 1 語で返す。

use super::{capture, search_region};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// transcript から読む末尾の上限（10 MiB）。
const TAIL_CAP: u64 = 10 * 1024 * 1024;
/// 健全な statusline の window の下限。
const WINDOW_FLOOR: u64 = 100_000;
/// 健全な statusline の使用率の上限。
const PCT_CEIL: u64 = 100;
/// 使用率の最大桁数。
const PCT_DIGITS: usize = 3;
/// token 数の最大桁数。
const TOKEN_DIGITS: usize = 9;

/// 出所が pane（statusline）であること。
pub const SOURCE_PANE: &str = "pane";
/// 出所が transcript（jsonl）であること。
pub const SOURCE_JSONL: &str = "jsonl";

/// pane 本文は得られたが statusline の候補行が無い。
pub const REASON_NO_STATUSLINE: &str = "pane-no-statusline";
/// statusline の候補は在るが健全性を外れている。
pub const REASON_OUT_OF_BOUND: &str = "pane-out-of-bound";
/// transcript は読めたが有効な usage が無い。
pub const REASON_JSONL_NO_USAGE: &str = "jsonl-no-usage";
/// tmux を撃てなかった。
pub const REASON_TMUX_FAILED: &str = "tmux-failed";
/// 出所が 1 つも成立しなかった。
pub const REASON_NO_SOURCE: &str = "no-source";

/// 計測 1 回の入力。
pub struct Request<'a> {
    /// tmux target（pane の指定）。
    pub target: &'a str,
    /// tmux の socket（既定の server を使うなら `None`）。
    pub socket: Option<&'a str>,
    /// pane 本文の代わりに読む file（在れば tmux を呼ばない）。
    pub capture_file: Option<&'a str>,
    /// fallback に使う transcript（明示のときだけ読む・C2.2）。
    pub transcript: Option<&'a str>,
}

/// 計測できた 1 回。取れなかった値は `None` で、`0` に化けさせない。
pub struct Reading {
    /// 使用率（%）。
    pub used_pct: Option<u64>,
    /// 使用 token 数。
    pub used_tokens: Option<u64>,
    /// window の token 数。
    pub window_tokens: Option<u64>,
    /// 成立した出所。
    pub source: &'static str,
}

/// 計測 1 回の結果。
pub enum Measure {
    /// 成立（出所付き）。
    Measured(Reading),
    /// 不成立（理由は 1 語）。
    Unmeasured(&'static str),
}

/// pane を出所として見た結果。
enum PaneLook {
    /// 健全な statusline が在る。
    Measured(u64, u64, u64),
    /// 候補は在るが健全性を外れている（**fallback しない**＝壊れた面を別の出所で塗らない）。
    OutOfBound,
    /// 本文は得られたが候補行が無い（fallback 可）。
    NoStatusline,
    /// 出所として成立しなかった（fallback 可・理由つき）。
    Absent(&'static str),
}

/// 計測を 1 回行う。
pub fn measure(request: &Request) -> Measure {
    match look_at_pane(request) {
        PaneLook::Measured(pct, used, window) => Measure::Measured(Reading {
            used_pct: Some(pct),
            used_tokens: Some(used),
            window_tokens: Some(window),
            source: SOURCE_PANE,
        }),
        PaneLook::OutOfBound => Measure::Unmeasured(REASON_OUT_OF_BOUND),
        PaneLook::NoStatusline => fallback(request, REASON_NO_STATUSLINE),
        PaneLook::Absent(reason) => fallback(request, reason),
    }
}

/// pane 本文を得て statusline を読む。
fn look_at_pane(request: &Request) -> PaneLook {
    let pane = match request.capture_file {
        // 明示された本文を読む周は tmux を 1 回も呼ばない。
        Some(path) => match std::fs::read_to_string(path) {
            Ok(found) => found,
            Err(_) => return PaneLook::Absent(REASON_NO_SOURCE),
        },
        None => match capture(request.socket, request.target) {
            Some(found) => found,
            None => return PaneLook::Absent(REASON_TMUX_FAILED),
        },
    };
    read_pane(&pane)
}

/// pane 本文から statusline を読む。候補が複数なら**最終行**を採る。
fn read_pane(pane: &str) -> PaneLook {
    let region = search_region(pane);
    let Some((pct, used, window)) = region.iter().rev().find_map(|line| parse_statusline(line))
    else {
        return if region.is_empty() {
            PaneLook::Absent(REASON_NO_SOURCE)
        } else {
            PaneLook::NoStatusline
        };
    };
    if pct <= PCT_CEIL && used <= window && window >= WINDOW_FLOOR {
        PaneLook::Measured(pct, used, window)
    } else {
        PaneLook::OutOfBound
    }
}

/// transcript を fallback として読む。**明示されていない周は読まない**（C2.2）。
fn fallback(request: &Request, pane_reason: &'static str) -> Measure {
    let Some(path) = request.transcript else {
        return Measure::Unmeasured(pane_reason);
    };
    match read_tail(Path::new(path)).as_deref().and_then(last_usage) {
        Some(used) => Measure::Measured(Reading {
            used_pct: None,
            used_tokens: Some(used),
            window_tokens: None,
            source: SOURCE_JSONL,
        }),
        None => Measure::Unmeasured(REASON_JSONL_NO_USAGE),
    }
}

/// 成立した 1 回を 1 行にする。取れなかった値は `-` で、`0` と区別する。
pub fn render(reading: &Reading) -> String {
    format!(
        "seat: meter used_pct={} used_tokens={} window_tokens={} source={}",
        shown(reading.used_pct),
        shown(reading.used_tokens),
        shown(reading.window_tokens),
        reading.source
    )
}

/// 不成立の 1 行。
pub fn render_unmeasured(reason: &str) -> String {
    format!("seat: meter unmeasured reason={reason}")
}

/// 値の表示。無い値は `-`。
fn shown(value: Option<u64>) -> String {
    value.map_or_else(|| "-".to_owned(), |found| found.to_string())
}

/// statusline 1 行を `(使用率, 使用 token, window token)` へ読む。
///
/// 形は `<pct>% <used>[kM]?/<window>[kM]` で、後ろは行末か空白である（v1 の
/// statusline と同じ規則）。合わない行は `None`＝候補ではない。
fn parse_statusline(line: &str) -> Option<(u64, u64, u64)> {
    let body = line.trim_start_matches([' ', '\t', '\u{b}', '\u{c}', '\r']);
    let (pct_text, rest) = body.split_once("% ")?;
    let pct = digits(pct_text, PCT_DIGITS)?;
    let (used_text, window_rest) = rest.split_once('/')?;
    let used = scaled(used_text, false)?;
    let window_text = window_rest.split(' ').next()?;
    let window = scaled(window_text, true)?;
    Some((pct, used, window))
}

/// 数字だけの列を 1〜`max` 桁で読む。
fn digits(text: &str, max: usize) -> Option<u64> {
    let count = text.chars().count();
    if count == 0 || count > max || !text.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    text.parse::<u64>().ok()
}

/// `<digits>[kM]` を読む。`k` は ×1000・`M` は ×1000000。
fn scaled(text: &str, suffix_required: bool) -> Option<u64> {
    let (head, scale) = match text.strip_suffix('k') {
        Some(found) => (found, 1_000_u64),
        None => match text.strip_suffix('M') {
            Some(found) => (found, 1_000_000_u64),
            None if suffix_required => return None,
            None => (text, 1_u64),
        },
    };
    digits(head, TOKEN_DIGITS)?.checked_mul(scale)
}

/// transcript の末尾 10 MiB を読む。
fn read_tail(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_CAP))).ok()?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer).ok()?;
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

/// jsonl から **最後の**有効な usage 和を取る。
fn last_usage(text: &str) -> Option<u64> {
    text.lines().filter_map(usage_of).next_back()
}

/// transcript 1 本から使用 token を読む（**失敗の理由を 1 語で弁別する**）。
///
/// [`measure`] の fallback 経路は `read_tail(...).and_then(last_usage)` と畳んでいるので、
/// 「file を読めない」と「有効な usage が 1 件も無い」が [`REASON_JSONL_NO_USAGE`] の 1 語へ
/// 潰れる。seat guard（`hook::seat_guard`）は測れなかった理由を記録に残す契約なので、
/// 同じ 2 段を**畳まずに**返す口をここへ 1 本置く（parse の実体は上の 2 関数のまま＝
/// 2 面目を作らない）。**[`measure`] の経路と出力は 1 byte も変えていない。**
pub(crate) fn used_from_transcript(path: &Path) -> Result<u64, &'static str> {
    let text = read_tail(path).ok_or("unreadable")?;
    last_usage(&text).ok_or("no-usage")
}

/// 1 行から usage の和を取る（assistant ∧ 非 sidechain ∧ usage object ∧ 和 > 0）。
fn usage_of(line: &str) -> Option<u64> {
    if !has_value(line, "type", "assistant") || has_value(line, "isSidechain", "true") {
        return None;
    }
    let usage = object_of(line, "usage")?;
    let sum = [
        "input_tokens",
        "cache_creation_input_tokens",
        "cache_read_input_tokens",
    ]
    .iter()
    .filter_map(|key| number_of(usage, key))
    // 欠落は 0 として足す。和が 0 の entry は「数えていない」＝無効である。
    .fold(0_u64, |total, found| total.saturating_add(found));
    (sum > 0).then_some(sum)
}

/// `"<key>"` が **key として**現れた occurrence の、値の先頭。
///
/// 値として現れた同綴りを拾わないために、直後の非空白が `:` である occurrence だけを
/// 採る（JSON では文字列の内側の `"` は escape されるので弁別できる・hook 側と同形）。
fn value_of<'a>(src: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!("\"{key}\"");
    let mut rest = src;
    loop {
        let (_, after) = rest.split_once(&needle)?;
        match after.trim_start().strip_prefix(':') {
            None => rest = after,
            Some(value) => return Some(value.trim_start()),
        }
    }
}

/// `"<key>"` の値が `expected` で始まる occurrence が 1 つでも在るか。
///
/// key の順序を仮定しない（`type` は top-level にも `message` の中にも在りうる）。
fn has_value(src: &str, key: &str, expected: &str) -> bool {
    let needle = format!("\"{key}\"");
    let mut rest = src;
    while let Some((_, after)) = rest.split_once(&needle) {
        if let Some(value) = after.trim_start().strip_prefix(':') {
            if token_of(value.trim_start()) == Some(expected) {
                return true;
            }
        }
        rest = after;
    }
    false
}

/// 値の字面を 1 語だけ取る（文字列は `"` の中身・素の値は区切りまで）。
///
/// 前方一致で見ると `assistant` が `assistant-x` に、`true` が `truely` に当たるので、
/// **語として**切り出してから比べる。
fn token_of(value: &str) -> Option<&str> {
    match value.strip_prefix('"') {
        Some(body) => body.split('"').next(),
        None => value.split([',', '}', ' ', '\t']).next(),
    }
}

/// `"<key>": { … }` の中身を対応する `}` まで返す。
fn object_of<'a>(src: &'a str, key: &str) -> Option<&'a str> {
    let value = value_of(src, key)?;
    if !value.starts_with('{') {
        return None;
    }
    let mut depth = 0_i64;
    for (at, ch) in value.char_indices() {
        match ch {
            '{' => depth = depth.saturating_add(1),
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return value.get(..=at);
                }
            }
            _ => {}
        }
    }
    None
}

/// `"<key>": <数字>` の値。
fn number_of(src: &str, key: &str) -> Option<u64> {
    let value = value_of(src, key)?;
    let text: String = value.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits(&text, TOKEN_DIGITS)
}
