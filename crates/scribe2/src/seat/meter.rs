//! context 使用量の計測（SRS FR25・設計 §3）。
//!
//! 出所は **transcript が名指された周は transcript・名指されない周は pane**（statusline）で、
//! **fallback は無い**（s2-07l.75）——出所が入力で決まる 1 本道にしないと、hook の中で pane を
//! 持てない guard（C2.2）と 2 面が同じ瞬間に違う値を返す。成立した出所を `source=` として必ず
//! 一緒に運ぶ（憲法 C10「Measured は出所付き」）。端末描画は出所の 1 つであって seat state の
//! 判定入力ではない（C3.3）。
//!
//! **計測できない周は 0% に化けない**（FR25）: 健全性を外れた statusline は捏造値を
//! 流さず不成立にし、理由を 1 語で返す。

use super::{capture, search_region};
use crate::rules::manifest::Manifest;
use crate::rules::RuleValue;
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
/// 出所が transcript（jsonl）と rules 行の**宣言窓**であること。
///
/// **3 値の出所が同じではない**ので 1 語で名乗らない（憲法 C10）——`used_tokens` は jsonl から
/// 測った値、`window_tokens` は rules 行 `seat.context_window_tokens` の**宣言値**、`used_pct` は
/// その 2 つからの**導出値**である。`jsonl` とだけ名乗ると 3 値が同じ出所と読める＝判定行は
/// 出所から切り離されて流通するので、読む側に伝わらない（planner 裁定 2026-09-11）。
/// pane 経路は 3 値とも statusline 由来ゆえ [`SOURCE_PANE`] のままでよい。
pub const SOURCE_JSONL_RULES: &str = "jsonl+rules";

/// pane 本文は得られたが statusline の候補行が無い。
pub const REASON_NO_STATUSLINE: &str = "pane-no-statusline";
/// statusline の候補は在るが健全性を外れている。
pub const REASON_OUT_OF_BOUND: &str = "pane-out-of-bound";
/// tmux を撃てなかった。
pub const REASON_TMUX_FAILED: &str = "tmux-failed";
/// 出所が 1 つも成立しなかった。
pub const REASON_NO_SOURCE: &str = "no-source";
/// 測るのに要る宣言（rules 行）が読めない＝**割る数が無い**ので測らない。
///
/// guard の cap 欠落と**同じ語**である（設計 §3「測れない理由は 4 語で弁別する」）。5 語目を
/// 足すと、記録の語彙が本便の都合で増える——出所を 1 本にする便が記録の形を動かさない。
pub const REASON_NO_RULE: &str = "no-rule";

/// 窓を宣言する rules 行の id（**値は code に焼かない**・憲法 C5）。
const ID_WINDOW: &str = "seat.context_window_tokens";
/// cap を宣言する rules 行の id（**値は code に焼かない**・憲法 C5）。
const ID_CAP: &str = "seat.context_cap_pct";
/// 百分率の分子。
const PERCENT: u64 = 100;

/// 計測 1 回の入力。
pub struct Request<'a> {
    /// tmux target（pane の指定）。
    pub target: &'a str,
    /// tmux の socket（既定の server を使うなら `None`）。
    pub socket: Option<&'a str>,
    /// pane 本文の代わりに読む file（在れば tmux を呼ばない）。
    pub capture_file: Option<&'a str>,
    /// 出所に使う transcript（**明示された周はこれだけを見る**・空文字は「無い」と同じ・C2.2）。
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

/// 計測を 1 回行う。
///
/// **transcript が名指された周は transcript だけを見る**（pane を混ぜない）。出所が入力で
/// 決まる 1 本道にしないと、同じ入力に 2 つの答えが在る状態が残る——hook の中の guard は
/// tmux も env も見ない（C2.2）ので pane を持てず、pane 一次のままだと guard と meter が
/// **同じ席の同じ瞬間に違う値**を返す（cap 60% が何に対する 60% か 1 か所で言えない）。
///
/// 実測 2026-09-11（両席 × 26 点対）: 2 つの出所は**同じ量**を見ている（差は最大 1.7% /
/// 平均 0.6%）。ただし pane は statusline の 1k 刻みの**階段**（22 点で異なり 4 種）で、
/// transcript は 1 token 粒度の**連続**（同 14 種）である。境界（cap）の判定は細かい側を
/// 見るほうが取り違えが少なく、**CC が圧縮する量そのもの**でもある（前席の実測: transcript
/// 976,065 = 97.6% で auto-compact が発火・statusline は 97%）。
pub fn measure(request: &Request) -> Measure {
    // 空文字は「無い」と同じ（trim 後）。渡し忘れが理由の取り違えにならないようにする。
    if let Some(path) = request.transcript.map(str::trim).filter(|p| !p.is_empty()) {
        return match used_from_transcript_pct(Path::new(path)) {
            Ok((pct, used, window)) => Measure::Measured(Reading {
                used_pct: Some(pct),
                used_tokens: Some(used),
                window_tokens: Some(window),
                source: SOURCE_JSONL_RULES,
            }),
            Err(reason) => Measure::Unmeasured(reason),
        };
    }
    match look_at_pane(request) {
        Ok((pct, used, window)) => Measure::Measured(Reading {
            used_pct: Some(pct),
            used_tokens: Some(used),
            window_tokens: Some(window),
            source: SOURCE_PANE,
        }),
        Err(reason) => Measure::Unmeasured(reason),
    }
}

/// pane 本文を得て statusline を読む。
fn look_at_pane(request: &Request) -> Result<(u64, u64, u64), &'static str> {
    let pane = match request.capture_file {
        // 明示された本文を読む周は tmux を 1 回も呼ばない。
        Some(path) => std::fs::read_to_string(path).map_err(|_| REASON_NO_SOURCE)?,
        None => capture(request.socket, request.target).ok_or(REASON_TMUX_FAILED)?,
    };
    used_from_pane_pct(&pane)
}

/// pane 本文（statusline）から**使用率まで**測る（`(使用率, 使用 token, 窓)`）。候補が複数なら
/// **最終行**を採る。
///
/// **[`measure`] と管理 tick が呼ぶ 1 本の口**である（`s2-07l.89`）。tick は idle 判定のために
/// 同じ本文を既に持っているので、ここへ渡す＝transcript の path を tick へ写す seam は作らない
/// （憲法 C10.3）。parse を 2 本にすると、片方だけが形を変えたときに guard / meter / tick の
/// 3 面が静かにずれる。
///
/// 健全性（pct ≤ 100 ∧ used ≤ window ∧ window ≥ 100000）を外れた候補は
/// [`REASON_OUT_OF_BOUND`] で**不成立のまま**（fallback しない＝壊れた面を別の出所で塗らない）。
/// 候補が無い周は本文が空なら [`REASON_NO_SOURCE`]・在れば [`REASON_NO_STATUSLINE`]。
pub fn used_from_pane_pct(pane: &str) -> Result<(u64, u64, u64), &'static str> {
    let region = search_region(pane);
    let Some((pct, used, window)) = region.iter().rev().find_map(|line| parse_statusline(line))
    else {
        return Err(if region.is_empty() {
            REASON_NO_SOURCE
        } else {
            REASON_NO_STATUSLINE
        });
    };
    if pct <= PCT_CEIL && used <= window && window >= WINDOW_FLOOR {
        Ok((pct, used, window))
    } else {
        Err(REASON_OUT_OF_BOUND)
    }
}

/// transcript を出所に**使用率まで**測る（`(使用率, 使用 token, 窓)`）。
///
/// **guard と meter が呼ぶ 1 本の口**である。2 つ目の計算を作ると、片方だけが丸めや窓を
/// 変えたときに 2 面が静かにずれる——本便が畳もうとしている当の穴になる。
///
/// 窓は **rules 行の宣言値**（測れる窓を transcript は持たない）。宣言が読めない周は
/// **割らずに不成立**にする（0 で割らない・0% に化けない）。
///
/// 使用率は**切り捨て**である。境界は「cap 以上で止める」ので、切り上げると cap 未満の
/// 周まで止まる（guard の従来の丸めをそのまま持ってきている）。
pub fn used_from_transcript_pct(path: &Path) -> Result<(u64, u64, u64), &'static str> {
    let used = used_from_transcript(path)?;
    let window = declared_window().ok_or(REASON_NO_RULE)?;
    Ok((used.saturating_mul(PERCENT) / window, used, window))
}

/// 埋め込みの宣言から窓を引く。
fn declared_window() -> Option<u64> {
    window_of(&Manifest::embedded().ok()?)
}

/// 埋め込みの宣言から cap を引く。**guard と管理 tick が呼ぶ 1 本の口**（`s2-07l.89`）。
///
/// cap の行 id を 2 か所で持たない: guard が private に持っていた読みをここへ移し、tick は
/// 自前の literal で cap を読まない。窓（[`window_of`]）と同じ module に置くのは、「何に対する
/// 60% か」（窓）と「60% とは何か」（cap）を同じ面が答えるためである。
pub fn declared_cap() -> Option<u64> {
    cap_of(&Manifest::embedded().ok()?)
}

/// 宣言（rules 行）から cap を引く。不発効・別の形は `None`（＝測らない側へ倒す）。
///
/// `0` を拒まないのは guard の従来の読み（`int_of`）をそのまま持ってきているため——cap 0 は
/// 「常に止める」の宣言であって欠落ではない（窓の `> 0` は 0 で割らないための条件で別物）。
pub fn cap_of(manifest: &Manifest) -> Option<u64> {
    let row = manifest.get(ID_CAP)?;
    match (row.enabled, &row.value) {
        (true, RuleValue::Int(found)) => Some(*found),
        _ => None,
    }
}

/// 宣言（rules 行）から窓を引く。不発効・別の形・0 は `None`（＝測らない側へ倒す）。
///
/// **manifest を引数で取る**のは、埋め込みを関数の中で呼ぶと 3 つの述語（行の有無 /
/// `enabled` / `> 0`）を歯から動かせなくなるためである（`Manifest::parse` は pub で、
/// `tests/e2e/fleet.rs` の `LockPolicy::from_rules` が同じ形の前例）。「0 で割らない」は
/// 本便が doc で名乗った保証なので、名乗る側が測れる形で置く。
pub fn window_of(manifest: &Manifest) -> Option<u64> {
    let row = manifest.get(ID_WINDOW)?;
    match (row.enabled, &row.value) {
        (true, RuleValue::Int(found)) if *found > 0 => Some(*found),
        _ => None,
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
/// **module private である**。外から呼べると「共有の口を通らずに自分で割る」形が書けてしまい、
/// 割る数がたまたま同じなら歯も通る——一致の歯が測るのは「同じ数」であって「同じ関数」では
/// ないので、可視性でしか塞げない（s2-07l.75 lens M-1）。使用率が要る面は
/// [`used_from_transcript_pct`] を呼ぶ。
///
/// 「file を読めない」（`unreadable`）と「有効な usage が 1 件も無い」（`no-usage`）を
/// **畳まずに**返す。seat guard は測れなかった理由を記録に残す契約（FR21）で、畳むと
/// 記録から原因を取り違える。
///
/// **この 2 語が 2 面の共通語である**。本便より前は、同じ条件を guard が `unreadable` /
/// `no-usage`、meter が `jsonl-no-usage` と**別の語で**呼んでいた——出所が 2 本あった状態の
/// もう 1 つの顔で、記録と CLI を突き合わせると同じ事象が別の名前で残っていた。
fn used_from_transcript(path: &Path) -> Result<u64, &'static str> {
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
