//! 作業記憶の消費（`seat consume`・設計 docs/design/working-memory.md §5.3・ADR-0018 §2.1 / §2.4・
//! SRS FR23 / AC8・憲法 C11.2 / N1.2）。
//!
//! 消費の実体は **move** である（削除しない）: 自席（frontmatter の `seat:` 一致）の未 consumed 退避物が
//! 1 件だけ在るとき、sid が現在と同じなら `working-memory.<sid>.consumed.md` へ移し、違うなら現在 sid の
//! 名義へ移して frontmatter の末尾に `consumed-from: <元 sid>` を 1 行足す（他の byte は変えない）。
//! **移し先を上書きしない**（既在は `consumed-exists`）。2 件以上は選ばずに止める（`wm-ambiguous`）。
//! 0 件で現在 sid の消費済みが在る周は冪等に rc 0（`already`）。

use super::{is_unconsumed_name, seat_dir, seat_of, state, StateDir, FRONTMATTER, WM_CONSUMED, WM_PREFIX, WM_SUFFIX};
use crate::polarity::{OnFailure, Polarity, Timing};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

/// sid が違う周に frontmatter へ足す key。
pub const CONSUMED_FROM_KEY: &str = "consumed-from:";

/// この境界の極性（[`ConsumeError`]）: move の前に判定し、測れない周・曖昧な周は 1 file も動かさない。
pub const POLARITY: Polarity = Polarity {
    timing: Timing::InLoop,
    on_failure: OnFailure::FailClosed,
};

/// 消費 1 回の入力。
pub struct Request<'a> {
    /// tmux target（frontmatter の `seat:` と突き合わせる）。
    pub target: &'a str,
    /// 退避物の dir。
    pub wm_dir: &'a Path,
    /// 解決済みの置き場（打刻を読む）。
    pub state_dir: &'a StateDir,
}

/// 消費を止める判定（**境界の enum**・[`POLARITY`]）。どの variant でも file は動かさない。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumeError {
    /// 置き場を解けない。
    StateDir,
    /// 打刻 file が無い。
    SidMissing,
    /// 打刻 file が読めない・最終行が壊れている・空。
    SidUnreadable,
    /// 最終の打刻の `sid` が空。
    SidEmpty,
    /// `sid` が file 名に使えない字を含む（dir の外を指す形を作らない）。
    SidInvalid,
    /// 退避物の dir を読めない（0 件と読み替えない）。
    WmUnreadable,
    /// 自席の未 consumed も現在 sid の消費済みも無い。
    WmMissing,
    /// 自席の未 consumed が 2 件以上（どれを消費するか選ばない）。
    WmAmbiguous(usize),
    /// 移し先が既に在る（上書きしない・N1）。
    ConsumedExists,
    /// frontmatter の閉じ区切りが無い（`consumed-from:` を足す位置を決められない）。
    FrontmatterUnclosed,
    /// 読めない・書けない。
    Unwritable,
}

impl ConsumeError {
    /// 断りの行の `reason=`。
    pub fn reason(&self) -> &'static str {
        match self {
            Self::StateDir => "state-dir",
            Self::SidMissing => "sid-missing",
            Self::SidUnreadable => "sid-unreadable",
            Self::SidEmpty => "sid-empty",
            Self::SidInvalid => "sid-invalid",
            Self::WmUnreadable => "wm-unreadable",
            Self::WmMissing => "wm-missing",
            Self::WmAmbiguous(_) => "wm-ambiguous",
            Self::ConsumedExists => "consumed-exists",
            Self::FrontmatterUnclosed => "frontmatter-unclosed",
            Self::Unwritable => "unwritable",
        }
    }
}

/// 消費の結果（stdout 1 行の材料）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Consumed {
    /// 移した（新しい名前・sid が違った周は元 sid）。
    Moved {
        /// 移し先の名前。
        file: String,
        /// sid が違った周の元 sid。
        from: Option<String>,
    },
    /// 対象が無く、現在 sid の消費済みが既に在る（冪等）。
    Already(String),
}

/// 成功の 1 行。
pub fn render(done: &Consumed) -> String {
    match done {
        Consumed::Moved { file, from: None } => format!("seat: consumed file={file}"),
        Consumed::Moved { file, from: Some(from) } => format!("seat: consumed file={file} consumed-from={from}"),
        Consumed::Already(file) => format!("seat: consumed already file={file}"),
    }
}

/// 断りの 1 行。
pub fn render_refused(err: &ConsumeError) -> String {
    let head = format!("seat: consume refused reason={}", err.reason());
    match err {
        ConsumeError::WmAmbiguous(count) => format!("{head} n={count}"),
        _ => head,
    }
}

/// 消費を 1 回行う。**判定を全部済ませてから** 1 回だけ動かす。
pub fn run(request: &Request) -> Result<Consumed, ConsumeError> {
    let sid = sid_of(&seat_dir(&request.state_dir.path, request.target))?;
    let consumed_name = format!("{WM_PREFIX}{sid}{WM_CONSUMED}");
    let mut own = candidates(request.wm_dir, request.target)?;
    let source = match own.len() {
        0 => {
            return if request.wm_dir.join(&consumed_name).exists() {
                Ok(Consumed::Already(consumed_name))
            } else {
                Err(ConsumeError::WmMissing)
            };
        }
        1 => own.pop().ok_or(ConsumeError::WmMissing)?,
        count => return Err(ConsumeError::WmAmbiguous(count)),
    };
    let dest = request.wm_dir.join(&consumed_name);
    if dest.symlink_metadata().is_ok() {
        return Err(ConsumeError::ConsumedExists);
    }
    let from = sid_of_name(&source).filter(|found| *found != sid);
    match &from {
        None => move_same(&source, &dest)?,
        Some(from) => move_with_origin(&source, &dest, from)?,
    }
    Ok(Consumed::Moved { file: consumed_name, from })
}

/// 自席の未 consumed 退避物の path（`scan_wm` と同じ名前と名乗りの条件）。
fn candidates(dir: &Path, target: &str) -> Result<Vec<PathBuf>, ConsumeError> {
    let entries = std::fs::read_dir(dir).map_err(|_| ConsumeError::WmUnreadable)?;
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_unconsumed_name(&name) {
            continue;
        }
        let path = entry.path();
        if seat_of(&path).is_some_and(|seat| seat == target) {
            found.push(path);
        }
    }
    Ok(found)
}

/// 退避物の名前が持つ sid（`working-memory.<sid>.md` の `<sid>`）。
fn sid_of_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    name.strip_prefix(WM_PREFIX)?
        .strip_suffix(WM_SUFFIX)
        .map(str::to_owned)
}

/// 打刻の最終行の `sid`（不在 / 読めない / 空 / file 名に使えない を分けて断る・externalize と同じ読み）。
fn sid_of(seat: &Path) -> Result<String, ConsumeError> {
    let text = match std::fs::read_to_string(state::path(seat)) {
        Ok(found) => found,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Err(ConsumeError::SidMissing),
        Err(_) => return Err(ConsumeError::SidUnreadable),
    };
    let line = text
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or(ConsumeError::SidUnreadable)?;
    let stamp = state::Stamp::from_line(line).map_err(|_| ConsumeError::SidUnreadable)?;
    let sid = stamp.sid.trim();
    if sid.is_empty() {
        return Err(ConsumeError::SidEmpty);
    }
    let safe = sid.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        && !sid.starts_with('.');
    if !safe {
        return Err(ConsumeError::SidInvalid);
    }
    Ok(sid.to_owned())
}

/// 同 sid の move。hard link で移し先を作る（**既在なら失敗する＝上書きしない**）→ 元を外す。
fn move_same(source: &Path, dest: &Path) -> Result<(), ConsumeError> {
    std::fs::hard_link(source, dest).map_err(|err| match err.kind() {
        std::io::ErrorKind::AlreadyExists => ConsumeError::ConsumedExists,
        _ => ConsumeError::Unwritable,
    })?;
    if std::fs::remove_file(source).is_err() {
        std::fs::remove_file(dest).ok();
        return Err(ConsumeError::Unwritable);
    }
    Ok(())
}

/// sid 違いの move。`consumed-from:` を足した中身を `create_new` で書き（既在なら失敗）→ 元を外す。
fn move_with_origin(source: &Path, dest: &Path, from: &str) -> Result<(), ConsumeError> {
    let body = std::fs::read(source).map_err(|_| ConsumeError::Unwritable)?;
    let body = with_origin(&body, from).ok_or(ConsumeError::FrontmatterUnclosed)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::AlreadyExists => ConsumeError::ConsumedExists,
            _ => ConsumeError::Unwritable,
        })?;
    let written = file.write_all(&body).and_then(|()| file.sync_all()).is_ok();
    if !written || std::fs::remove_file(source).is_err() {
        std::fs::remove_file(dest).ok();
        return Err(ConsumeError::Unwritable);
    }
    Ok(())
}

/// 先頭の `---` 区間の**閉じ区切りの直前**に `consumed-from: <from>` を 1 行差し込む（他の byte は不変）。
/// 先頭が区切りでない・閉じ区切りが無いなら `None`。
pub fn with_origin(body: &[u8], from: &str) -> Option<Vec<u8>> {
    let mut offset = 0_usize;
    let mut first = true;
    for line in body.split_inclusive(|b| *b == b'\n') {
        let is_fence = std::str::from_utf8(line).is_ok_and(|text| text.trim() == FRONTMATTER);
        if first {
            if !is_fence {
                return None;
            }
            first = false;
        } else if is_fence {
            let (head, tail) = body.split_at(offset);
            let mut out = Vec::with_capacity(body.len().saturating_add(from.len()).saturating_add(20));
            out.extend_from_slice(head);
            // 閉じ区切りの前の行が改行で終わらない形は無い（split_inclusive の途中の行は必ず `\n` で終わる）。
            out.extend_from_slice(format!("{CONSUMED_FROM_KEY} {from}\n").as_bytes());
            out.extend_from_slice(tail);
            return Some(out);
        }
        offset = offset.saturating_add(line.len());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seat_wm_consume_inserts_origin_before_the_closing_fence_only() {
        let body = b"---\nschema: 1\nseat: wm:1\n---\n\n## body\n---\nseat: other\n";
        let Some(out) = with_origin(body, "sid-a") else {
            panic!("閉じ区切りが在る");
        };
        assert_eq!(
            out,
            b"---\nschema: 1\nseat: wm:1\nconsumed-from: sid-a\n---\n\n## body\n---\nseat: other\n".to_vec()
        );
    }

    #[test]
    fn seat_wm_consume_refuses_to_insert_without_frontmatter_fences() {
        assert_eq!(with_origin(b"# no front\nseat: x\n", "s"), None, "先頭が区切りでない");
        assert_eq!(with_origin(b"---\nseat: x\n", "s"), None, "閉じ区切りが無い");
    }

    #[test]
    fn seat_wm_consume_keeps_crlf_bytes_around_the_inserted_line() {
        let Some(out) = with_origin(b"---\r\nseat: x\r\n---\r\nbody\r\n", "s1") else {
            panic!("CRLF でも区切りを読む");
        };
        assert_eq!(out, b"---\r\nseat: x\r\nconsumed-from: s1\n---\r\nbody\r\n".to_vec());
    }

    #[test]
    fn seat_wm_consume_refusal_lines_name_reason_and_count() {
        assert_eq!(render_refused(&ConsumeError::WmAmbiguous(2)), "seat: consume refused reason=wm-ambiguous n=2");
        assert_eq!(render_refused(&ConsumeError::ConsumedExists), "seat: consume refused reason=consumed-exists");
        assert_eq!(render(&Consumed::Already("f".to_owned())), "seat: consumed already file=f");
    }
}
