//! `seat` subcommand の面（設計 docs/design/seat-autonomy.md §3）: 開発 session の
//! context 使用量を読む [`meter`] と、tmux pane へ 1 行を送る [`inject`]。
//!
//! **env も HOME も読まない**（憲法 C2.2）。pane は `--target`（または明示された
//! `--capture-file`）だけを見て、transcript は `--transcript` で明示された file だけを
//! 読む。tmux は外部 process の呼出しで、crate 依存は増えない（憲法 A3 非該当）。
//!
//! 出力は行を組んで返すだけで、stdout / stderr へは bin 側の `emit` / `emit_err` が
//! 書く（憲法 C2）。

pub mod cli;
pub mod inject;
pub mod meter;

use std::process::Command;

/// 開発 session の入力欄を指す prompt の字。
///
/// pane の読みはこの字を **anchor** に使う（statusline は最後の prompt 行より下に
/// 描かれ、打ちかけの text はその右に在る）。
pub const PROMPT: char = '❯';

/// tmux を 1 回撃って stdout を得る。起動失敗・rc 非 0 はいずれも `None`。
fn tmux_stdout(socket: Option<&str>, args: &[&str]) -> Option<String> {
    let mut command = Command::new("tmux");
    if let Some(path) = socket {
        command.arg("-S").arg(path);
    }
    let out = command.args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// tmux を 1 回撃ち、成功したかだけを見る（出力を持たない send 系に使う）。
pub fn tmux_ok(socket: Option<&str>, args: &[&str]) -> bool {
    let mut command = Command::new("tmux");
    if let Some(path) = socket {
        command.arg("-S").arg(path);
    }
    command
        .args(args)
        .output()
        .is_ok_and(|out| out.status.success())
}

/// pane 本文を capture する。撃てなければ `None`（**空文字と区別する**）。
pub fn capture(socket: Option<&str>, target: &str) -> Option<String> {
    tmux_stdout(socket, &["capture-pane", "-p", "-t", target])
}

/// 最後の prompt 行の右（入力欄）の字面。prompt 行が無ければ `None`。
///
/// `None` は「入力欄が空」ではなく **特定できない**である（呼び側は fail-closed に
/// 倒す＝送らない）。
pub fn input_tail(pane: &str) -> Option<&str> {
    pane.lines()
        .rfind(|line| line.contains(PROMPT))?
        .split_once(PROMPT)
        .map(|(_, right)| right.trim())
}

/// statusline を探す域: 最後の prompt 行より下の非空行（prompt 不在なら末尾 6 非空行）。
pub fn search_region(pane: &str) -> Vec<&str> {
    let lines: Vec<&str> = pane.lines().collect();
    match lines.iter().rposition(|line| line.contains(PROMPT)) {
        Some(at) => lines
            .iter()
            .skip(at.saturating_add(1))
            .filter(|line| !line.trim().is_empty())
            .copied()
            .collect(),
        None => {
            let mut tail: Vec<&str> = lines
                .iter()
                .rev()
                .filter(|line| !line.trim().is_empty())
                .take(TAIL_LINES)
                .copied()
                .collect();
            tail.reverse();
            tail
        }
    }
}

/// prompt が 1 行も無い pane で末尾から見る行数。
const TAIL_LINES: usize = 6;

/// target を file 名に使える字面へ潰す（`[A-Za-z0-9_.-]` 以外は `_`）。
///
/// **潰した結果が `.` か `..` になった周は全部 `_` にする**。この 2 形は path の
/// component として上へ抜けるので、`<state_dir>/seat/<target>/tick.jsonl` が
/// `<state_dir>/tick.jsonl` になり **state dir の外を書く**（多段の `../x` は `.._x` に
/// 潰れるので抜けない）。長さは変えない（何文字を潰したかを残す）。
pub fn sanitize_target(target: &str) -> String {
    let squashed: String = target
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if squashed == "." || squashed == ".." {
        return "_".repeat(squashed.len());
    }
    squashed
}
