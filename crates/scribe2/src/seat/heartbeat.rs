//! 席が「tick を処理した」ことを打刻する面（設計 §3・裁定 (a)）。
//!
//! 打刻は席の**中**から撃たれる 1 回で、tick（席の外）の鮮度判定の入力になる。置き場を
//! anchor の `.claude-session/` でなく `<state_dir>/seat/<target>/` に取るのは、v1 の
//! timer と場所を分けて併走できるようにするためである（設計 §3）。

use super::sanitize_target;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// 打刻 file の名前。
pub const FILE: &str = "heartbeat";
/// 置き場を解けない。
pub const REASON_STATE_DIR: &str = "state-dir";
/// 打刻 file を書けない。
pub const REASON_UNWRITABLE: &str = "unwritable";

/// 打刻 file の path。
pub fn path(seat_dir: &Path) -> PathBuf {
    seat_dir.join(FILE)
}

/// 打刻する（無ければ作る）。
///
/// **開くだけでは mtime は進まない**ので、明示的に現在時刻を書く。進まないと 2 回目の
/// 打刻が鮮度に効かず、席が生きていることを tick から読めない。
pub fn touch(seat_dir: &Path) -> std::io::Result<()> {
    touch_at(&path(seat_dir))
}

/// 名指した marker を打刻する（tick の自打刻もこの口を通る＝打刻は 1 実装）。
pub fn touch_at(marker: &Path) -> std::io::Result<()> {
    if let Some(dir) = marker.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = OpenOptions::new().create(true).append(true).open(marker)?;
    File::set_modified(&file, SystemTime::now())
}

/// 打刻した 1 行。表示は記録の dir 名と同じ**潰した字面**にする（語彙を 1 つにする）。
pub fn render(target: &str) -> String {
    format!("seat: heartbeat target={}", sanitize_target(target))
}

/// 置き場を解けなかった 1 行。
pub fn render_refused(reason: &str) -> String {
    format!("seat: heartbeat refused reason={reason}")
}
