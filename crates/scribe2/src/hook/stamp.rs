//! 席の状態の打刻（hook 側・設計 docs/design/seat-state.md §2 / §3・ADR-0015 §2.2 / §2.3）。
//!
//! `UserPromptSubmit` / `Stop` / `SessionStart` の周に、生成 hooks.json の shell 行が渡す `--pane`
//! （`$TMUX_PANE` の展開は shell が行う）から tmux の target を解き、core の
//! [`state::append`] を呼ぶ。**guard ではない**（行為を止めない・極性一覧に載せない・設計 §5）:
//! `--pane` が無い・空（tmux の外）・target を解けない・`Stop` の再入（`stop_hook_active`）は
//! いずれも打刻せず黙る。書けなかった周も席を止めない（rc 0・stdout 0 byte）が、黙って消さず
//! stderr へ 1 行 surface する。**env は読まない**（憲法 C2.2）。

use super::{bool_field, field, flag_of};
use crate::seat::state::{self, Event, Stamp};
use std::path::Path;

/// payload から拾う key（この session の id）。
const KEY_SESSION_ID: &str = "session_id";
/// payload から拾う key（`Stop` hook の再入）。
const KEY_STOP_ACTIVE: &str = "stop_hook_active";
/// 自席の pane id を渡す flag。
const FLAG_PANE: &str = "--pane";
/// tmux の socket を渡す flag（tick と同じ・歯は独立 socket で撃つ）。
const FLAG_SOCKET: &str = "--tmux-socket";

/// 打刻を 1 回試みる。戻りは stderr へ載せる行（黙る周・書けた周は空）。
pub fn stamp(args: &[String], payload: &str, event: Event, state_dir: &Path) -> Vec<String> {
    // `Stop` の再入だけを黙る（他の event の payload に同じ key が在っても打刻を止めない）。
    if event == Event::Stop && bool_field(payload, KEY_STOP_ACTIVE) == Some(true) {
        return Vec::new();
    }
    let Some(pane) = flag_of(args, FLAG_PANE).filter(|found| !found.trim().is_empty()) else {
        return Vec::new();
    };
    let socket = flag_of(args, FLAG_SOCKET).filter(|found| !found.trim().is_empty());
    let Some(target) = crate::seat::target_of_pane(socket, pane) else {
        return Vec::new();
    };
    let sid = field(payload, KEY_SESSION_ID).unwrap_or_default();
    let stamp = Stamp::now(event, &sid);
    match state::append(&crate::seat::seat_dir(state_dir, &target), &stamp) {
        Ok(warnings) => warnings.iter().map(|w| w.as_str().to_owned()).collect(),
        Err(err) => vec![err.to_string()],
    }
}
