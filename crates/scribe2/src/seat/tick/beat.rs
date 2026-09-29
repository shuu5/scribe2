//! heartbeat の実効の値（設計 docs/design/seat-heartbeat.md §22・契約表の行 aa・FR78・ADR-0092）。
//!
//! 合図を送るかは、明示の記録（席の置き場の `heartbeat-off` / `heartbeat-on`）→ 群の表の行の key `heartbeat`（anchor が群の行か
//! 区画の行の anchors に在る周）→ 置き場の種類の既定（群は on・区画は off・どの行にも無い置き場は on）の順で決まり、どこで決まった
//! かを閉じた 3 語 [`By`] で名乗る。明示の記録が無く群の表を読めない周は値も決まり方も `unreadable`（合図を送らない側）。
//! 明示の記録の読み書きもこの file が持つ（口 [`super::heartbeat`] と判定の列 `front` の同じ 1 本）。
//!
//! この file は判定の列の値（`NoopReason` と `SeatCommand`）を名指さない（他の行の touches の閉包を広げない）。

use crate::hook::group::{group_of, park_of};
use crate::rules::manifest::{Heartbeat, Manifest};
use crate::seat::state;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// 明示 off の file 名（席の置き場の直下・1 行 `ts=<UTC 秒>`・既に在る停止の記録がそのまま明示 off・設計 §12 形 1）。
pub const OFF_FILE: &str = "heartbeat-off";
/// 明示 on の file 名（席の置き場の直下・1 行 `ts=<UTC 秒>`・設計 §22 形 2）。
pub const ON_FILE: &str = "heartbeat-on";

/// 在るのに読めない値の字面（`-` に潰さない）。
const UNREADABLE: &str = "unreadable";

/// 群の表の読み（3 形）。
#[derive(Debug, Clone, Copy)]
pub enum Table<'a> {
    /// 読めた面（群の行・区画の行を持つ manifest）。
    Read(&'a Manifest),
    /// 面の無い周（host の面の file が無い＝どの行にも属さない）。
    Absent,
    /// 面が読めない周。
    Unreadable,
}

/// 実効の値（**閉じた 3 値**・字面は口と doctor の `heartbeat=` の語）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value {
    /// 合図を送る。
    On,
    /// 合図を送らない。
    Off,
    /// 明示の記録が無く面を読めない（合図を送らない側）。
    Unreadable,
}

impl Value {
    /// 出力の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
            Self::Unreadable => UNREADABLE,
        }
    }
}

/// 決まり方（**閉じた 3 語 + 読めない**・宣言順が決まる順・C2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum By {
    /// 明示の記録。
    Explicit,
    /// 群の表の行の key。
    Group,
    /// 置き場の種類の既定。
    Default,
    /// 明示の記録が無く面を読めない。
    Unreadable,
}

impl By {
    /// 出力の語。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Group => "group",
            Self::Default => "default",
            Self::Unreadable => UNREADABLE,
        }
    }
}

/// 実効の値と決まり方（1 組で運ぶ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Beat {
    /// 実効の値。
    pub value: Value,
    /// 決まり方。
    pub by: By,
}

impl Beat {
    /// 合図を送らない側か（off と unreadable）。
    pub fn silent(self) -> bool {
        self.value != Value::On
    }
}

/// 明示 off の path。
pub fn off_path(seat: &Path) -> PathBuf {
    seat.join(OFF_FILE)
}

/// 明示 on の path。
pub fn on_path(seat: &Path) -> PathBuf {
    seat.join(ON_FILE)
}

/// file が在るか（NotFound だけが無い・在るのに読めない周〔dir・権限〕も在る）。
fn present(path: &Path) -> bool {
    !matches!(fs::symlink_metadata(path), Err(err) if err.kind() == ErrorKind::NotFound)
}

/// 明示の記録の読み（設計 §22 形 2）: 明示 off が在れば（読めない・dir・両方在る周を含む）off、明示 on が読める file なら on、在るのに
/// 読めない明示 on は off、両方無ければ `None`（次の段へ）。
fn explicit(seat: &Path) -> Option<Value> {
    if present(&off_path(seat)) {
        return Some(Value::Off);
    }
    let on = on_path(seat);
    present(&on).then(|| if fs::read_to_string(&on).is_ok() { Value::On } else { Value::Off })
}

/// 実効の値の 1 関数（設計 §22 形 3）: 明示の記録 → 群の表の行の key（anchor が群の行か区画の行の anchors に在り、その行が key を
/// 持つ周・group）→ 種類の既定（群は on・区画は off・どの行にも無い置き場は on・default）。明示の記録が無く面が読めない周は
/// 値も決まり方も unreadable。
pub fn resolve(seat: &Path, anchor: &str, table: Table) -> Beat {
    if let Some(value) = explicit(seat) {
        return Beat { value, by: By::Explicit };
    }
    let manifest = match table {
        Table::Read(manifest) => manifest,
        Table::Absent => return Beat { value: Value::On, by: By::Default },
        Table::Unreadable => return Beat { value: Value::Unreadable, by: By::Unreadable },
    };
    let value_of = |set: Option<Heartbeat>, kind: Value| match set {
        Some(Heartbeat::On) => Beat { value: Value::On, by: By::Group },
        Some(Heartbeat::Off) => Beat { value: Value::Off, by: By::Group },
        None => Beat { value: kind, by: By::Default },
    };
    match (group_of(manifest, anchor), park_of(manifest, anchor)) {
        (Some(row), _) => value_of(row.heartbeat(), Value::On),
        (None, Some(lot)) => value_of(lot.heartbeat(), Value::Off),
        (None, None) => Beat { value: Value::On, by: By::Default },
    }
}

/// 記録 1 つを置く（既に在れば触らない・席の置き場は登録 row の在る席にだけ作る・一時 file → rename）。
fn put(seat: &Path, name: &str) -> std::io::Result<()> {
    if present(&seat.join(name)) {
        return Ok(());
    }
    fs::create_dir_all(seat)?;
    let temporary = seat.join(format!("{name}.tmp"));
    fs::write(&temporary, format!("ts={}\n", state::now_secs()))?;
    fs::rename(&temporary, seat.join(name))
}

/// 記録 1 つを消す（無い周は何もしない）。
fn remove(seat: &Path, name: &str) -> std::io::Result<()> {
    match fs::remove_file(seat.join(name)) {
        Err(err) if err.kind() != ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

/// `seat heartbeat on`（設計 §22 形 5）: 明示 off を消してから明示 on を置く（在れば ts を書き換えない）。
pub fn set_on(seat: &Path) -> std::io::Result<()> {
    remove(seat, OFF_FILE)?;
    put(seat, ON_FILE)
}

/// `seat heartbeat off`（設計 §22 形 5）: 明示 off を置いてから明示 on を消す（在れば触らない）。
pub fn set_off(seat: &Path) -> std::io::Result<()> {
    put(seat, OFF_FILE)?;
    remove(seat, ON_FILE)
}

/// `seat heartbeat default`（設計 §22 形 5）: 明示の記録を両方消す（無ければ何もしない）。
pub fn set_default(seat: &Path) -> std::io::Result<()> {
    remove(seat, OFF_FILE)?;
    remove(seat, ON_FILE)
}
