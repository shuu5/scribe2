//! 群の逼迫の読み手（設計 docs/design/account-lifecycle.md §19・契約表の行 h・ADR-0055）。
//!
//! 閾値の rules 行 3 本（窓ごとに 1 行・[`Caps`]）と、口座 1 つの最新の実測から逼迫を判じる 1 本（[`pressed`]）を持つ。
//! 読み手は 2 つで、**同じこの 1 本を通る**（C2）: dispatch の 1 周の群の段（`pipe::dispatch` の `group`）と、席自身の
//! hook（[`lines`]・SessionStart と UserPromptSubmit）。
//!
//! hook は自席の登録 row の口座 1 つだけを読み、**鮮度の外は hook の中で測らない**（`hook.timeout_s` < `fleet.usage_timeout_s`）:
//! 器自身を子として `fleet usage --state-dir D --account <label> --fresh` で起こして待たず、`usage: measuring` の 1 行を出す
//! （値は次の話す番で読める）。群に属さない anchor・群 0 の host は 1 語も出さない。移動は作らない（第 3 段・§20）。

use super::{record, record_lines, Emit, Hooked};
use crate::fleet::usage;
use crate::fleet::{Allowance, WindowKind};
use crate::rules::manifest::{AccountGroup, Manifest};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

/// 5 時間窓の閾値の rules 行（値は manifest だけが持つ・C1 / C5）。
const ROW_FIVE: &str = "fleet.group_pressure_5h_pct";

/// 7 日窓の閾値の rules 行。
const ROW_SEVEN: &str = "fleet.group_pressure_7d_pct";

/// モデル別 7 日窓の閾値の rules 行。
const ROW_MODEL: &str = "fleet.group_pressure_model_pct";

/// 記録の `what`（逼迫の 1 行・計測の子を起こした 1 行）。
const WHAT_GROUP: &str = "group-pressure";

/// 窓ごとの閾値（使用率の百分率・rules 行 3 本の値）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// 5 時間窓。
    five: u64,
    /// 7 日窓。
    seven: u64,
    /// モデル別 7 日窓。
    model: u64,
}

impl Caps {
    /// 3 行を読む。どれかが無い・不発効・整数でない周は理由つきの `Err`（既定の値を焼かない・C1）。
    pub fn of(manifest: &Manifest) -> Result<Self, String> {
        Ok(Self {
            five: crate::rules::int_row(manifest, ROW_FIVE)?,
            seven: crate::rules::int_row(manifest, ROW_SEVEN)?,
            model: crate::rules::int_row(manifest, ROW_MODEL)?,
        })
    }

    /// 窓に当てる行の値（**窓ごとに別の行**・網羅 match）。
    fn cap(self, window: WindowKind) -> u64 {
        match window {
            WindowKind::FiveHour => self.five,
            WindowKind::SevenDay => self.seven,
            WindowKind::SevenDayModel => self.model,
        }
    }
}

/// 逼迫と判じた 1 窓（閾値を越えた窓のうち使用率が最大の 1 つ）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pressed {
    /// 窓。
    pub window: WindowKind,
    /// 使用率（整数 %）。
    pub used: u64,
    /// その窓の閾値の行の値。
    pub cap: u64,
}

/// 口座 1 つの最新の実測の行から逼迫を判じる（**pure**・判定の 1 本）。実測の窓のどれかの `used_pct` が対応する行の値
/// **以上**なら、越えた窓のうち使用率が最大の 1 つ（同率は行の並び＝窓の宣言順で先の 1 つ）。測れなかった行は数えない
/// （0 に読み替えない・越えたとも言わない）。
pub fn pressed(rows: &[Allowance], caps: Caps) -> Option<Pressed> {
    rows.iter()
        .filter_map(|row| match row {
            Allowance::Measured(found) => Some(found),
            Allowance::Unmeasured(_) => None,
        })
        .map(|found| Pressed { window: found.window, used: found.used_pct, cap: caps.cap(found.window) })
        .filter(|found| found.used >= found.cap)
        .fold(None, |best: Option<Pressed>, found| match best {
            Some(kept) if kept.used >= found.used => Some(kept),
            _ => Some(found),
        })
}

/// anchor の属する群（宣言順で最初の 1 つ・宣言は同じ置き場を 2 つの群に置けない）。
pub fn group_of<'a>(manifest: &'a Manifest, anchor: &str) -> Option<&'a AccountGroup> {
    manifest.groups().iter().find(|group| group.anchors().iter().any(|found| found == anchor))
}

/// hook の 1 行（`group=<名> account=<label> window=<w> used=<n> cap=<n> — …`・設計 §19 形 5）。
fn seat_line(group: &AccountGroup, account: &str, found: Pressed) -> String {
    format!(
        "group={} account={account} window={} used={} cap={} — 移動は次の 1 周（第 3 段まで手で）",
        group.name(),
        found.window.short(),
        found.used,
        found.cap
    )
}

/// 席の hook の群の段（SessionStart の brief・UserPromptSubmit の追加文脈に足す行）。`who` / `when` は記録の欄。
///
/// 群に属さない anchor・群 0 の host・登録の無い席・読めない面は 0 行（席は止めない・rc は変えない）。鮮度の内側で逼迫なら
/// 1 行、閾値未満なら 0 行、鮮度の外なら計測の子を 1 本起こして `usage: measuring account=<label>` の 1 行。出した行は
/// 記録 1 行（`what` = [`WHAT_GROUP`]）を残す。
pub(super) fn lines(hooked: &Hooked, (who, when): (&str, &str), started: Instant) -> (Vec<String>, Vec<String>) {
    let Some(line) = line_of(hooked) else {
        return (Vec::new(), Vec::new());
    };
    let emit = Emit { who, what: WHAT_GROUP, when, line: &line };
    (vec![line.clone()], record_lines(hooked.dir, &record(&emit, hooked, started)))
}

/// 群の段の 1 行（出さない周は `None`）。
fn line_of(hooked: &Hooked) -> Option<String> {
    let tracked = hooked.rules.map_or_else(Manifest::embedded, |path| Manifest::load(Path::new(path))).ok()?;
    let manifest = crate::rules::with_state_dir(tracked, Some(hooked.dir)).ok()?;
    let group = group_of(&manifest, &hooked.root.display().to_string())?;
    let caps = Caps::of(&manifest).ok()?;
    let pane = hooked.pane.filter(|found| !found.trim().is_empty())?;
    let socket = hooked.socket.filter(|found| !found.trim().is_empty());
    let target = crate::seat::target_of_pane(socket, pane)?;
    let events = crate::fleet::store::read_all(hooked.dir).ok()?;
    let state = crate::fleet::replay(&events);
    let account = crate::seat::role::registration_of_target(&state, &target)?.account.clone();
    match usage::fresh_rows(&manifest, &state, &account).ok()? {
        Some(rows) => pressed(&rows, caps).map(|found| seat_line(group, &account, found)),
        None => measure_later(hooked.dir, &account).then(|| format!("usage: measuring account={account}")),
    }
}

/// 鮮度の外の口座を**子として**測る（`fleet usage --state-dir D --account <label> --fresh` の 1 形・待たない）。子は新しい
/// process group の leader にし（hook の終わりで道連れにしない）、入出力は捨てる。起こせた周だけ `true`。
fn measure_later(state_dir: &Path, account: &str) -> bool {
    Command::new(crate::pipe::dispatch::myself())
        .args(["fleet", "usage", "--state-dir"])
        .arg(state_dir)
        .args(["--account", account, "--fresh"])
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::{pressed, Caps, Pressed};
    use crate::fleet::{Allowance, Measured, WindowKind};

    /// 実測の行 1 つ。
    fn measured(window: WindowKind, used_pct: u64) -> Allowance {
        Allowance::Measured(Measured {
            account: "a1".to_owned(),
            window,
            model: (window == WindowKind::SevenDayModel).then(|| "Fable".to_owned()),
            endpoint: "oauth-usage".to_owned(),
            used_pct,
            resets_at: None,
        })
    }

    /// 越えた窓が 2 つ在れば使用率の大きい方、同率は窓の宣言順で先の 1 つ。越えない窓は使用率が大きくても選ばない。
    #[test]
    fn hook_group_pressed_picks_the_largest_window_over_its_own_cap() {
        let caps = Caps { five: 85, seven: 95, model: 95 };
        let rows =
            [measured(WindowKind::FiveHour, 90), measured(WindowKind::SevenDay, 94), measured(WindowKind::SevenDayModel, 97)];
        assert_eq!(pressed(&rows, caps), Some(Pressed { window: WindowKind::SevenDayModel, used: 97, cap: 95 }));
        let tie = [measured(WindowKind::FiveHour, 96), measured(WindowKind::SevenDay, 96)];
        assert_eq!(pressed(&tie, caps).map(|found| found.window), Some(WindowKind::FiveHour), "同率は先の窓");
        let under = [measured(WindowKind::FiveHour, 84), measured(WindowKind::SevenDay, 94)];
        assert_eq!(pressed(&under, caps), None, "どの窓も閾値未満");
    }
}
