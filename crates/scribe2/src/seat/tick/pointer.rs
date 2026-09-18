//! 打刻の合図の**梯子**（設計 docs/design/seat-autonomy.md §14・`s2-07l.423`・SRS FR27 / FR29）。
//!
//! 打刻の合図の brake は `s2-07l.109` 以来 **時間だけ**（tick-stamp の mtime が `seat.tick_stale_s`
//! 未満なら送らない・[`super::pointer_recent`]）で、席の状態が前回の合図から変わったかを見なかった。
//! 承認待ちで 6 時間無変化の席に同じ合図が 40 分ごとに約 30 回届き、各回が 1 turn を消費した
//! （folio2 planner の実測 2026-09-17・user 裁定 2026-09-17T00:55Z）。この module は「変化」を
//! typed に測り（[`Digest`]）、無変化の席への合図の間隔を段ごとに伸ばして最後は止める梯子の
//! **記録**（[`Ladder`]）と**待ちの計算**（[`wait_s`]）を持つ。判定そのものは [`super::pointer_brake`]。
//!
//! **判定入力は typed な log の最終行だけ**（憲法 C3.3・席の描画や自由文は見ない）: 材料は 2 つ＝
//! 席の状態 log（`seat/<target>/state.jsonl`）の最終行の `ts` と、置き場の fleet の event log
//! （`fleet/events.jsonl`）の最終行の `ts`。**context は入れない**（合図に応える turn ごとに増えるので
//! 無変化の席でも毎回変わる）。**台帳は読まない**（毎周 `bd` を子 process で撃つ費用を tick に
//! 持ち込まない・台帳の動きは席の turn になって状態 log に現れる）。digest は 2 値を空白で並べた
//! 1 行で、hash にしない（読める形で残す・C10）。
//!
//! **「読めない」を値に潰さない**（憲法 C10 / C11）: 測れなかった側は語 [`UNREADABLE`] を置く
//! （0 や空に化けさせない）。記録が無い・読めない周は [`Record::Absent`] で、梯子ではなく
//! **従来どおり tick-stamp の brake だけ**で判定する（記録を書けない席へ tick の周期で合図が
//! 重ならない・fail-open の面は「40 分に合図 1 本」に留まる）。

use crate::fleet::json_lite::{self, Value};
use crate::fleet::store;
use crate::seat::state;
use std::fmt;
use std::io::{Read as _, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// 梯子の記録 file の名前（tick-stamp の隣・1 行 JSON）。
pub(super) const RECORD_FILE: &str = "pointer-digest";
/// 測れなかった材料の語（**0 や空に化けさせない**・憲法 C10）。
const UNREADABLE: &str = "unreadable";
/// 末尾 1 行を読むために end から遡る窓（byte）。log を**全読みしない**（設計 §14 形 1）。
const TAIL_BYTES: u64 = 64 * 1024;

/// 記録の key: 合図を送った時刻（1970 年からの秒・UTC）。
const KEY_SENT_AT: &str = "sent_at";
/// 記録の key: 送った段（0 = 初段＝待ちは `seat.tick_stale_s`）。
const KEY_STEP: &str = "step";
/// 記録の key: 基準の digest（settle 前は `null`）。
const KEY_DIGEST: &str = "digest";

/// 席の変化の digest（設計 §14 形 1）。**2 値で閉じる**——材料を増やすと、合図に応える turn
/// そのものが「変化」に数えられて梯子が登らなくなる（context を入れない理由と同じ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Digest {
    /// 席の状態 log の最終行の `ts`（読めない周は [`UNREADABLE`]）。
    state_ts: String,
    /// 置き場の fleet の event log の最終行の `ts`（同上）。
    fleet_ts: String,
}

impl fmt::Display for Digest {
    /// 2 値を宣言順に空白 1 つで並べた 1 行（hash にしない＝記録から読める・C10）。
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{} {}", self.state_ts, self.fleet_ts)
    }
}

/// いまの digest を測る（`seat_dir` = 席の置き場・`state_dir` = 置き場の根）。
pub(super) fn digest_of(seat_dir: &Path, state_dir: &Path) -> Digest {
    Digest {
        state_ts: state_ts(seat_dir).map_or_else(|| UNREADABLE.to_owned(), |ts| ts.to_string()),
        fleet_ts: fleet_ts(state_dir).unwrap_or_else(|| UNREADABLE.to_owned()),
    }
}

/// 席の状態 log の最終行の `ts`（読めない・壊れている周は `None`）。
///
/// 読み手は既存の [`state::Stamp::from_line`] を使う（`state.rs` に第 2 の読み口を作らない）。
/// [`state::Read`] は `Busy(Event)` / `Idle(Event)` で `ts` を持たないので、ここで最終行から引く。
pub(super) fn state_ts(seat_dir: &Path) -> Option<u64> {
    let line = last_line(&state::path(seat_dir))?;
    state::Stamp::from_line(&line).ok().map(|stamp| stamp.ts)
}

/// fleet の event log の最終行の `ts`（`YYYY-MM-DDTHH:MM:SSZ` の文字列・読めない周は `None`）。
fn fleet_ts(state_dir: &Path) -> Option<String> {
    let line = last_line(&store::events_path(state_dir))?;
    let pairs = json_lite::parse_object(&line).ok()?;
    pairs
        .iter()
        .find(|(key, _)| key == "ts")
        .and_then(|(_, value)| value.as_str())
        .map(str::to_owned)
}

/// file の**末尾の非空行**。全読みしない（設計 §14 形 1）: 末尾の窓（[`TAIL_BYTES`]）だけを読み、
/// 窓の頭が行の途中なら最初の改行までを捨てる（半端な行を値にしない）。
fn last_line(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let window = len.min(TAIL_BYTES);
    let from = len.saturating_sub(window);
    file.seek(SeekFrom::Start(from)).ok()?;
    let mut buf = vec![0_u8; usize::try_from(window).ok()?];
    file.read_exact(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    let tail = if from == 0 {
        text.as_str()
    } else {
        text.split_once('\n').map(|(_, rest)| rest)?
    };
    tail.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(str::to_owned)
}

/// 梯子の記録 1 行（`seat/<target>/pointer-digest`）。**書く形**であり、読みは [`Record`] で閉じる。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Ladder {
    /// 合図を送った時刻（1970 年からの秒・UTC）。梯子の待ちはこの時刻から測る。
    pub(super) sent_at: u64,
    /// 送った段。
    pub(super) step: u32,
    /// 基準の digest。**送った周は `None`**（settle 前）——合図に応える席の turn が状態 log を必ず
    /// 1 行進めるので、送出時の digest を基準にすると毎回「変化あり」になり梯子が登らない。
    pub(super) digest: Option<String>,
}

impl Ladder {
    /// 1 行の flat JSON にする。
    pub(super) fn to_line(&self) -> String {
        json_lite::write_object(&[
            (KEY_SENT_AT, Value::Num(self.sent_at)),
            (KEY_STEP, Value::Num(u64::from(self.step))),
            (KEY_DIGEST, self.digest.clone().map_or(Value::Null, Value::Str)),
        ])
    }

    /// 1 行を読む。key の欠落・型違いはいずれも `None`（黙って別物を通さない）。
    fn from_line(line: &str) -> Option<Self> {
        let pairs = json_lite::parse_object(line.trim()).ok()?;
        let field = |key: &str| {
            pairs
                .iter()
                .find(|(found, _)| found == key)
                .map(|(_, value)| value)
        };
        let sent_at = field(KEY_SENT_AT)?.as_num()?;
        let step = u32::try_from(field(KEY_STEP)?.as_num()?).ok()?;
        let digest = match field(KEY_DIGEST)? {
            Value::Null => None,
            Value::Str(found) => Some(found.clone()),
            Value::Num(_) | Value::Bool(_) => return None,
        };
        Some(Self { sent_at, step, digest })
    }
}

/// 記録の読み。**閉じた 3 値**（憲法 C11・設計 §14 形 2）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Record {
    /// 記録が無い・読めない・壊れている。**settle 待ちに落とさず**、従来どおり tick-stamp の
    /// brake だけで判定する（段は 0・待ちの起点は tick-stamp の mtime）。
    Absent,
    /// 記録は在るが基準がまだ無い＝**settle 前**（送った周の記録）。
    Settling {
        /// 合図を送った時刻。
        sent_at: u64,
        /// 送った段。
        step: u32,
    },
    /// 基準が在る＝以後の周は今の digest と比べる。
    Based {
        /// 合図を送った時刻。
        sent_at: u64,
        /// 送った段。
        step: u32,
        /// 基準の digest。
        base: String,
    },
}

impl Record {
    /// 記録の段（記録なしは 0）。床で止まった周の判定行が載せる段である（設計 §14 形 4）。
    pub(super) fn step(&self) -> u32 {
        match *self {
            Self::Absent => 0,
            Self::Settling { step, .. } | Self::Based { step, .. } => step,
        }
    }
}

/// 記録の path。
pub(super) fn record_path(seat_dir: &Path) -> PathBuf {
    seat_dir.join(RECORD_FILE)
}

/// 記録を書く。**書けない周も合図は送る**（呼び側は結果を捨ててよい）＝次の周は記録なしの
/// 縮退へ倒れ、床（tick-stamp）が 40 分に 1 本の上限を守る。
pub(super) fn write(seat_dir: &Path, ladder: &Ladder) -> std::io::Result<()> {
    std::fs::write(record_path(seat_dir), format!("{}\n", ladder.to_line()))
}

/// 記録を読む。
pub(super) fn read(seat_dir: &Path) -> Record {
    std::fs::read_to_string(record_path(seat_dir)).map_or(Record::Absent, |text| record_of_line(&text))
}

/// 記録の 1 行を読みに写す（**file を読まない**＝判定を置き場から切り離す・in-file の歯の入口）。
pub(super) fn record_of_line(line: &str) -> Record {
    let Some(ladder) = Ladder::from_line(line) else {
        return Record::Absent;
    };
    match ladder.digest {
        None => Record::Settling { sent_at: ladder.sent_at, step: ladder.step },
        Some(base) => Record::Based { sent_at: ladder.sent_at, step: ladder.step, base },
    }
}

/// 段 `step` の待ち（秒）= `stale_s × factor ^ step`（設計 §14 形 2・初段は既存の
/// `seat.tick_stale_s` を流用して行を増やさない）。**溢れる段は `None`** ＝上限を超える側へ倒す
/// （`u64` を回り込ませて短い待ちに化けさせない）。
pub(super) fn wait_s(stale_s: u64, factor: u64, step: u32) -> Option<u64> {
    factor.checked_pow(step).map(|times| stale_s.saturating_mul(times))
}

#[cfg(test)]
mod tests {
    use super::{record_of_line, wait_s, Ladder, Record};

    /// (e) 待ちの計算: 初段は `stale_s` そのまま・1 段ごとに `factor` 倍・溢れる段は `None`
    /// （停止側へ倒す）。`factor` が 1 の周は永久に初段のまま（梯子が登らない値も値として通る）。
    #[test]
    fn pointer_ladder_wait_doubles_per_step_and_stops_on_overflow() {
        assert_eq!(wait_s(2400, 2, 0), Some(2400), "初段は seat.tick_stale_s そのまま");
        assert_eq!(wait_s(2400, 2, 1), Some(4800), "1 段で factor 倍");
        assert_eq!(wait_s(2400, 2, 5), Some(76_800), "5 段目（上限 86400 の内側）");
        assert_eq!(wait_s(2400, 2, 6), Some(153_600), "6 段目（上限の外＝呼び側が停止と読む）");
        assert_eq!(wait_s(2400, 2, 64), None, "factor^step が u64 を溢れる段は None");
        assert_eq!(wait_s(2400, 1, 9), Some(2400), "factor = 1 は登らない（値のまま通す）");
        assert_eq!(
            wait_s(u64::MAX, 2, 1),
            Some(u64::MAX),
            "積は飽和させる（回り込んで短い待ちに化けさせない）"
        );
    }

    /// (f) 記録の往復: 書いた 1 行は同じ 3 値で読め、`digest` の有無が settle 前と基準ありを
    /// 分ける。**壊れた行・key の欠けた行・型の違う行はすべて「記録なし」**（段 0）で、
    /// settle 待ちにも基準にも化けない（憲法 C11）。
    #[test]
    fn pointer_ladder_record_round_trips_and_broken_line_reads_as_absent() {
        let sent = Ladder { sent_at: 1_757_600_000, step: 3, digest: None };
        assert_eq!(
            record_of_line(&sent.to_line()),
            Record::Settling { sent_at: 1_757_600_000, step: 3 },
            "送った周の記録は settle 前"
        );
        let based = Ladder {
            sent_at: 1_757_600_000,
            step: 3,
            digest: Some("1757600001 2026-09-17T00:55:00Z".to_owned()),
        };
        assert_eq!(
            record_of_line(&based.to_line()),
            Record::Based {
                sent_at: 1_757_600_000,
                step: 3,
                base: "1757600001 2026-09-17T00:55:00Z".to_owned(),
            },
            "基準を書いた記録は基準あり（digest は空白区切りの 2 値のまま読める）"
        );
        let broken = [
            "壊れた行",
            "",
            r#"{"sent_at":1,"step":2}"#,
            r#"{"step":2,"digest":null}"#,
            r#"{"sent_at":"1","step":2,"digest":null}"#,
            r#"{"sent_at":1,"step":2,"digest":7}"#,
        ];
        for line in broken {
            assert_eq!(record_of_line(line), Record::Absent, "{line}");
            assert_eq!(record_of_line(line).step(), 0, "記録なしの段は 0: {line}");
        }
    }
}
